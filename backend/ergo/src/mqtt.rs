//! MQTT broker connection used by `mqtt.publish`.
//!
//! The broker comes from the `mqtt_url` option: a URL, or `auto` for the
//! Supervisor's service discovery (the Mosquitto add-on). MQTT is turned on
//! only once ergo has connected to it; otherwise ergo runs without MQTT.
//!
//! For the MQTT page, the last messages ergo published are kept in memory,
//! together with what arrives on a topic filter the page is watching.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use ergo_nodes::MqttPublisher;
use rumqttc::{AsyncClient, ConnectionError, Event, MqttOptions, Packet, QoS};
use serde::Serialize;
use serde_json::Value;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct MqttStatus {
    pub configured: bool,
    pub connected: bool,
    pub broker: Option<String>,
    pub error: Option<String>,
}

pub struct Mqtt {
    client: Option<AsyncClient>,
    broker: Option<String>,
    connected: AtomicBool,
    error: RwLock<Option<String>>,
    monitor: Mutex<Monitor>,
}

/// Messages kept for the MQTT page; the oldest go first.
const MONITOR_CAP: usize = 1000;
/// Longer payloads are cut, so a stray firmware image can't fill memory.
const PAYLOAD_CAP: usize = 16 * 1024;
/// A watch nobody has looked at for this long is stopped.
const WATCH_IDLE: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct MqttMessage {
    pub seq: u64,
    pub at: String,
    /// `sent` by ergo, or `received` on the watched filter.
    pub direction: &'static str,
    pub topic: String,
    pub payload: String,
    /// The payload isn't UTF-8 and is shown as hex.
    pub binary: bool,
    pub truncated: bool,
    pub bytes: usize,
    pub qos: u8,
    pub retain: bool,
}

#[derive(Default)]
struct Monitor {
    seq: u64,
    messages: VecDeque<MqttMessage>,
    filter: Option<String>,
    last_seen: Option<Instant>,
}

impl Monitor {
    fn push(
        &mut self,
        direction: &'static str,
        topic: &str,
        payload: &[u8],
        qos: u8,
        retain: bool,
    ) {
        self.seq += 1;
        let cut = &payload[..payload.len().min(PAYLOAD_CAP)];
        let (text, binary) = match std::str::from_utf8(cut) {
            Ok(t) => (t.to_string(), false),
            // A cut may split a character; keep the valid prefix as text.
            Err(e) if e.error_len().is_none() => (
                String::from_utf8_lossy(&cut[..e.valid_up_to()]).into_owned(),
                false,
            ),
            Err(_) => (
                cut.iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                true,
            ),
        };
        if self.messages.len() == MONITOR_CAP {
            self.messages.pop_front();
        }
        self.messages.push_back(MqttMessage {
            seq: self.seq,
            at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            direction,
            topic: topic.to_string(),
            payload: text,
            binary,
            truncated: payload.len() > PAYLOAD_CAP,
            bytes: payload.len(),
            qos,
            retain,
        });
    }
}

/// What the MQTT page polls: messages after `seq`, and the watch state.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorPage {
    pub status: MqttStatus,
    pub filter: Option<String>,
    pub seq: u64,
    pub messages: Vec<MqttMessage>,
}

fn qos_level(q: QoS) -> u8 {
    match q {
        QoS::AtMostOnce => 0,
        QoS::AtLeastOnce => 1,
        QoS::ExactlyOnce => 2,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Broker {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Broker {
    pub fn parse(url: &str) -> Result<Self> {
        let u = url::Url::parse(url).with_context(|| format!("invalid MQTT URL {url}"))?;
        if !matches!(u.scheme(), "mqtt" | "tcp") {
            bail!(
                "unsupported MQTT scheme {} (TLS arrives in a later version)",
                u.scheme()
            );
        }
        Ok(Self {
            host: u.host_str().context("MQTT URL has no host")?.to_string(),
            port: u.port().unwrap_or(1883),
            username: Some(u.username())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            password: u.password().map(str::to_string),
        })
    }

    /// Asks the Supervisor for the MQTT service (e.g. the Mosquitto add-on).
    pub async fn discover(supervisor_token: &str) -> Result<Option<Self>> {
        let res = reqwest::Client::new()
            .get("http://supervisor/services/mqtt")
            .bearer_auth(supervisor_token)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if !res.status().is_success() {
            return Ok(None);
        }
        let body: Value = res.json().await?;
        let d = &body["data"];
        let Some(host) = d["host"].as_str() else {
            return Ok(None);
        };
        if d["ssl"] == true {
            warn!(
                "the discovered MQTT broker requires TLS, which this version doesn't support yet"
            );
            return Ok(None);
        }
        Ok(Some(Self {
            host: host.to_string(),
            port: d["port"].as_u64().unwrap_or(1883) as u16,
            username: d["username"].as_str().map(str::to_string),
            password: d["password"].as_str().map(str::to_string),
        }))
    }
}

impl Mqtt {
    pub fn disabled(reason: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            client: None,
            broker: None,
            connected: AtomicBool::new(false),
            error: RwLock::new(reason),
            monitor: Mutex::default(),
        })
    }

    /// Connects to `broker`, waiting up to `wait` for it to accept, and then
    /// keeps the connection up in the background (reconnecting as needed).
    /// Fails if the broker can't be reached in time or turns the login down,
    /// so MQTT is only turned on with a broker that works.
    pub async fn connect(broker: Broker, wait: Duration) -> Result<Arc<Self>, String> {
        let mut opts = MqttOptions::new(
            format!("ergo-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
            broker.host.clone(),
            broker.port,
        );
        opts.set_keep_alive(Duration::from_secs(30));
        if let Some(user) = &broker.username {
            opts.set_credentials(user.clone(), broker.password.clone().unwrap_or_default());
        }
        let (client, mut eventloop) = AsyncClient::new(opts, 64);
        let mqtt = Arc::new(Self {
            client: Some(client),
            broker: Some(format!("{}:{}", broker.host, broker.port)),
            connected: AtomicBool::new(false),
            error: RwLock::new(None),
            monitor: Mutex::default(),
        });

        let refused = Arc::new(AtomicBool::new(false));
        let this = mqtt.clone();
        let refused_flag = refused.clone();
        let event_loop = tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        info!(broker = ?this.broker, "connected to MQTT broker");
                        this.connected.store(true, Ordering::Relaxed);
                        *this.error.write().unwrap() = None;
                        // Clean sessions forget subscriptions; renew the watch.
                        let filter = this.monitor.lock().unwrap().filter.clone();
                        if let (Some(f), Some(c)) = (filter, &this.client) {
                            let _ = c.try_subscribe(f, QoS::AtMostOnce);
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        this.monitor.lock().unwrap().push(
                            "received",
                            &p.topic,
                            &p.payload,
                            qos_level(p.qos),
                            p.retain,
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if matches!(e, ConnectionError::ConnectionRefused(_)) {
                            refused_flag.store(true, Ordering::Relaxed);
                        }
                        if this.connected.swap(false, Ordering::Relaxed) {
                            warn!(error = %e, "MQTT connection lost");
                        }
                        *this.error.write().unwrap() = Some(e.to_string());
                        // rumqttc reconnects on the next poll.
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        let deadline = Instant::now() + wait;
        while !mqtt.connected.load(Ordering::Relaxed) {
            if refused.load(Ordering::Relaxed) || Instant::now() >= deadline {
                event_loop.abort();
                let error = mqtt.error.read().unwrap().clone();
                return Err(match error {
                    Some(e) => format!(
                        "couldn't connect to {}: {e}",
                        mqtt.broker.as_deref().unwrap_or("?")
                    ),
                    None => format!(
                        "{} didn't answer within {} s",
                        mqtt.broker.as_deref().unwrap_or("the broker"),
                        wait.as_secs()
                    ),
                });
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // Stops a watch the page left behind (closed tab, lost connection).
        let this = mqtt.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let idle = {
                    let m = this.monitor.lock().unwrap();
                    m.filter.is_some() && m.last_seen.is_none_or(|t| t.elapsed() > WATCH_IDLE)
                };
                if idle {
                    info!("stopping the MQTT watch; the page isn't open");
                    let _ = this.watch(None).await;
                }
            }
        });
        Ok(mqtt)
    }

    /// Messages after `after`; also keeps the current watch alive.
    pub fn messages(&self, after: u64) -> MonitorPage {
        let mut m = self.monitor.lock().unwrap();
        m.last_seen = Some(Instant::now());
        MonitorPage {
            status: self.status(),
            filter: m.filter.clone(),
            seq: m.seq,
            messages: m
                .messages
                .iter()
                .filter(|x| x.seq > after)
                .cloned()
                .collect(),
        }
    }

    pub fn clear(&self) {
        self.monitor.lock().unwrap().messages.clear();
    }

    /// Subscribes to `filter` instead of the current one; `None` stops watching.
    pub async fn watch(&self, filter: Option<String>) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Err("no MQTT broker configured".into());
        };
        if let Some(f) = &filter {
            validate_filter(f)?;
        }
        let old = {
            let mut m = self.monitor.lock().unwrap();
            m.last_seen = Some(Instant::now());
            std::mem::replace(&mut m.filter, filter.clone())
        };
        if old == filter {
            return Ok(());
        }
        if let Some(old) = old {
            client.unsubscribe(old).await.map_err(|e| e.to_string())?;
        }
        if let Some(f) = filter {
            client
                .subscribe(f, QoS::AtMostOnce)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn status(&self) -> MqttStatus {
        MqttStatus {
            configured: self.client.is_some(),
            connected: self.connected.load(Ordering::Relaxed),
            broker: self.broker.clone(),
            error: self.error.read().unwrap().clone(),
        }
    }
}

#[async_trait]
impl MqttPublisher for Mqtt {
    async fn publish(
        &self,
        topic: &str,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
    ) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Err("no MQTT broker configured".into());
        };
        if !self.connected.load(Ordering::Relaxed) {
            return Err("MQTT broker not connected".into());
        }
        let level = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        let record = payload.clone();
        client
            .publish(topic, level, retain, payload)
            .await
            .map_err(|e| e.to_string())?;
        self.monitor
            .lock()
            .unwrap()
            .push("sent", topic, &record, qos_level(level), retain);
        Ok(())
    }
}

/// Checks a subscription filter the way brokers do, for a clear error here.
fn validate_filter(f: &str) -> Result<(), String> {
    if f.is_empty() {
        return Err("the topic filter is empty".into());
    }
    let levels: Vec<&str> = f.split('/').collect();
    for (i, level) in levels.iter().enumerate() {
        if level.contains('#') && (*level != "#" || i != levels.len() - 1) {
            return Err("# must be the last level on its own, as in home/#".into());
        }
        if level.contains('+') && *level != "+" {
            return Err("+ must be a whole level, as in home/+/state".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Broker, Monitor, PAYLOAD_CAP, validate_filter};

    #[test]
    fn checks_topic_filters() {
        assert!(validate_filter("#").is_ok());
        assert!(validate_filter("home/+/state").is_ok());
        assert!(validate_filter("home/#").is_ok());
        assert!(validate_filter("home/#/x").is_err());
        assert!(validate_filter("home/a+").is_err());
        assert!(validate_filter("").is_err());
    }

    #[test]
    fn records_payloads() {
        let mut m = Monitor::default();
        m.push("sent", "a", b"{\"on\":true}", 0, false);
        m.push("received", "b", &[0xff, 0x00], 1, true);
        m.push("received", "c", &vec![b'x'; PAYLOAD_CAP + 5], 0, false);
        let v: Vec<_> = m.messages.iter().collect();
        assert_eq!(v[0].payload, "{\"on\":true}");
        assert!(v[1].binary && v[1].payload == "ff 00" && v[1].retain);
        assert!(
            v[2].truncated && v[2].payload.len() == PAYLOAD_CAP && v[2].bytes == PAYLOAD_CAP + 5
        );
        assert_eq!(v[2].seq, 3);
    }

    #[test]
    fn parses_broker_urls() {
        assert_eq!(
            Broker::parse("mqtt://u:p@10.0.0.2:1884").unwrap(),
            Broker {
                host: "10.0.0.2".into(),
                port: 1884,
                username: Some("u".into()),
                password: Some("p".into())
            }
        );
        assert_eq!(Broker::parse("mqtt://localhost").unwrap().port, 1883);
        assert!(Broker::parse("mqtts://x").is_err());
    }
}
