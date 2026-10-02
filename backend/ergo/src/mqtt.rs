//! MQTT broker connection used by `mqtt.publish` and `trigger.mqtt`.
//!
//! The broker comes from the `mqtt_url` option: a URL, or `auto` for the
//! Supervisor's service discovery (the Mosquitto add-on). MQTT is turned on
//! only once ergo has connected to it; otherwise ergo runs without MQTT.
//!
//! One connection serves everything: publishing, the topics that active
//! workflows' MQTT triggers listen on, and the MQTT page, which keeps the last
//! messages ergo sent and received in memory and can watch any topic filter.

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use ergo_nodes::{MqttPublisher, filter_covers, topic_matches, valid_topic_filter};
use rumqttc::{AsyncClient, ConnectionError, Event, MqttOptions, Packet, QoS};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

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
    /// Filters subscribed for MQTT triggers (none covering another).
    triggers: Mutex<BTreeSet<String>>,
    /// Every message received, for the trigger dispatcher.
    incoming: broadcast::Sender<Incoming>,
    /// The last message that matched more than one subscription, to drop the
    /// copies a broker may send for each of them.
    last_overlap: Mutex<Option<(Instant, String, Vec<u8>)>>,
}

/// A message from the broker.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub topic: String,
    pub payload: Vec<u8>,
    pub qos: u8,
    /// Sent because it was the topic's retained message when we subscribed.
    pub retain: bool,
}

/// Every subscription asks for QoS 2. A broker delivers at the lower of the
/// publisher's QoS and the subscription's, so messages arrive (and show on
/// the MQTT page) at the QoS they were sent with, and triggers fire once.
const SUBSCRIBE_QOS: QoS = QoS::ExactlyOnce;

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
            triggers: Mutex::default(),
            incoming: broadcast::channel(1).0,
            last_overlap: Mutex::default(),
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
        let (client, mut eventloop) = AsyncClient::new(opts, 256);
        let mqtt = Arc::new(Self {
            client: Some(client),
            broker: Some(format!("{}:{}", broker.host, broker.port)),
            connected: AtomicBool::new(false),
            error: RwLock::new(None),
            monitor: Mutex::default(),
            triggers: Mutex::default(),
            incoming: broadcast::channel(1024).0,
            last_overlap: Mutex::default(),
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
                        // Clean sessions forget subscriptions; renew them all.
                        this.resubscribe();
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => this.on_publish(&p),
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

    fn resubscribe(&self) {
        let Some(client) = &self.client else { return };
        let triggers = self.triggers.lock().unwrap().clone();
        for f in &triggers {
            if let Err(e) = client.try_subscribe(f.clone(), SUBSCRIBE_QOS) {
                warn!(filter = %f, error = %e, "couldn't subscribe for an MQTT trigger");
            }
        }
        let watch = self.monitor.lock().unwrap().filter.clone();
        if let Some(f) = watch.filter(|f| !triggers.contains(f)) {
            let _ = client.try_subscribe(f, SUBSCRIBE_QOS);
        }
    }

    fn on_publish(&self, p: &rumqttc::Publish) {
        // Overlapping subscriptions (say home/# and home/+/state) may each
        // get a copy; keep the first.
        let matching = {
            let watch = self.monitor.lock().unwrap().filter.clone();
            let triggers = self.triggers.lock().unwrap();
            watch
                .iter()
                .chain(triggers.iter())
                .filter(|f| topic_matches(f, &p.topic))
                .count()
        };
        if matching > 1 {
            let mut last = self.last_overlap.lock().unwrap();
            if let Some((at, topic, payload)) = &*last
                && at.elapsed() < Duration::from_millis(250)
                && *topic == p.topic
                && payload[..] == p.payload[..]
            {
                debug!(topic = %p.topic, "dropped a duplicate from overlapping subscriptions");
                return;
            }
            *last = Some((Instant::now(), p.topic.clone(), p.payload.to_vec()));
        }
        self.monitor.lock().unwrap().push(
            "received",
            &p.topic,
            &p.payload,
            qos_level(p.qos),
            p.retain,
        );
        // No receivers is fine: no MQTT trigger is active.
        let _ = self.incoming.send(Incoming {
            topic: p.topic.clone(),
            payload: p.payload.to_vec(),
            qos: qos_level(p.qos),
            retain: p.retain,
        });
    }

    /// Messages as they arrive, for MQTT triggers.
    pub fn subscribe(&self) -> broadcast::Receiver<Incoming> {
        self.incoming.subscribe()
    }

    /// Subscribes to exactly what active MQTT triggers need. Filters covered
    /// by another one (home/a under home/#) aren't subscribed separately.
    pub fn set_trigger_filters(&self, wanted: impl IntoIterator<Item = String>) {
        let Some(client) = &self.client else { return };
        let wanted: BTreeSet<String> = wanted.into_iter().collect();
        let needed: BTreeSet<String> = wanted
            .iter()
            .filter(|f| !wanted.iter().any(|o| o != *f && filter_covers(o, f)))
            .cloned()
            .collect();
        let watch = self.monitor.lock().unwrap().filter.clone();
        let mut current = self.triggers.lock().unwrap();
        for f in current.difference(&needed) {
            // The page may still be watching it.
            if watch.as_deref() != Some(f.as_str()) {
                let _ = client.try_unsubscribe(f.clone());
            }
        }
        for f in needed.difference(&current) {
            if let Err(e) = client.try_subscribe(f.clone(), SUBSCRIBE_QOS) {
                warn!(filter = %f, error = %e, "couldn't subscribe for an MQTT trigger");
            }
        }
        if *current != needed {
            info!(filters = ?needed, "MQTT trigger subscriptions");
        }
        *current = needed;
    }

    /// The newest message received on a topic `filter` matches, if any is
    /// still in memory; a run started by hand replays it.
    pub fn latest_received(&self, filter: &str) -> Option<MqttMessage> {
        self.monitor
            .lock()
            .unwrap()
            .messages
            .iter()
            .rev()
            .find(|m| m.direction == "received" && topic_matches(filter, &m.topic))
            .cloned()
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
            valid_topic_filter(f)?;
        }
        let old = {
            let mut m = self.monitor.lock().unwrap();
            m.last_seen = Some(Instant::now());
            std::mem::replace(&mut m.filter, filter.clone())
        };
        if old == filter {
            return Ok(());
        }
        // Filters MQTT triggers need stay subscribed (at their QoS).
        let triggers = self.triggers.lock().unwrap().clone();
        if let Some(old) = old.filter(|f| !triggers.contains(f)) {
            client.unsubscribe(old).await.map_err(|e| e.to_string())?;
        }
        if let Some(f) = filter.filter(|f| !triggers.contains(f)) {
            client
                .subscribe(f, SUBSCRIBE_QOS)
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

#[cfg(test)]
mod tests {
    use super::{Broker, Monitor, PAYLOAD_CAP};

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
