//! MQTT broker connection used by `mqtt.publish`.
//!
//! The broker comes from `ERGO_MQTT_URL`, or, as an add-on, from the
//! Supervisor's service discovery (the Mosquitto add-on). No broker is fine:
//! ergo still runs, and publish nodes fail with a clear error.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use ergo_nodes::MqttPublisher;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
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
        })
    }

    /// Creates the client and drives its event loop in the background.
    pub fn connect(broker: Broker) -> Arc<Self> {
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
        });

        let this = mqtt.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        info!(broker = ?this.broker, "connected to MQTT broker");
                        this.connected.store(true, Ordering::Relaxed);
                        *this.error.write().unwrap() = None;
                    }
                    Ok(_) => {}
                    Err(e) => {
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
        mqtt
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
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        client
            .publish(topic, qos, retain, payload)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Broker;

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
