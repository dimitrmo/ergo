//! Node implementations. Each one is a [`NodeExecutor`] with its schema.
//!
//! Nodes that need the outside world (MQTT, HA) take it as a small trait, so
//! they can be tested with fakes and wired to real clients in the binary.

mod data;
mod flow;
mod ha;
mod http;
mod jsonata;
mod mqtt;
mod parse;
mod push;
mod text;

#[cfg(test)]
mod pipeline_tests;

pub use data::{DataFilter, DataMap};
pub use flow::{FlowDelay, FlowIf, FlowWait};
pub use ha::{ActionCall, HaAction, HaCaller, HaNotify};
pub use http::{HttpDownload, HttpRequest};
pub use mqtt::{MqttTrigger, filter_covers, topic_matches, valid_topic_filter};
pub use parse::{DataParse, JsonParser, Parser, XmlParser, parsers};
pub use push::{
    PushReport, PushSend, PushStore, PushSubscription, TITLE as PUSH_TITLE, URGENCIES, WebPush,
};
pub use text::TextCompose;

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use croner::Cron;
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema,
    Registry, RunCtx,
};
use serde_json::{Value, json};

/// Publishes MQTT messages; implemented by the binary's broker connection.
#[async_trait]
pub trait MqttPublisher: Send + Sync {
    async fn publish(
        &self,
        topic: &str,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
    ) -> Result<(), String>;
}

/// Every node type. Without `mqtt` (MQTT turned off), the MQTT trigger and
/// publish are left out and workflows that use them are told why.
pub fn registry(
    mqtt: Option<Arc<dyn MqttPublisher>>,
    ha: Arc<dyn HaCaller>,
    push: Arc<WebPush>,
) -> Registry {
    const MQTT_OFF: &str =
        "MQTT is off. Check the add-on's mqtt_url option; the Status page says why.";
    let mut r = Registry::default();
    r.register(StateTrigger);
    r.register(CronTrigger);
    r.register(ManualTrigger);
    match mqtt {
        Some(mqtt) => {
            r.register(MqttTrigger);
            r.register(MqttPublish { mqtt });
        }
        None => {
            r.disable("trigger.mqtt", MQTT_OFF);
            r.disable("mqtt.publish", MQTT_OFF);
        }
    }
    r.register(HaAction::new(ha.clone()));
    r.register(HaNotify::new(ha.clone()));
    r.register(FlowIf::new(ha.clone()));
    r.register(FlowDelay);
    r.register(FlowWait::new(ha));
    r.register(PushSend::new(push));
    r.register(TextCompose);
    r.register(HttpDownload::new());
    r.register(HttpRequest::new());
    r.register(DataParse::new());
    r.register(DataFilter);
    r.register(DataMap);
    r
}

/// Triggers pass the event that fired them on: it's their input and output.
fn trigger_output(input: &Value) -> Result<NodeOutput, NodeError> {
    Ok(NodeOutput::out(input.clone()))
}

/// JSON Schema helpers for the output shapes below.
pub(crate) fn string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

pub(crate) fn object(props: Value) -> Value {
    json!({ "type": "object", "properties": props })
}

/// Fields every trigger event has.
fn event_fields(extra: Value) -> Value {
    let mut props = json!({
        "kind": string("What started the run: state, cron, mqtt or manual"),
        "time": string("When it fired, in HA's time zone"),
        "node": string("The trigger's node id"),
        "id": string("The trigger's nickname, if set"),
    });
    if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    object(props)
}

pub struct StateTrigger;

#[async_trait]
impl NodeExecutor for StateTrigger {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("trigger.state", NodeKind::Trigger, "State change")
            .description("Fires when an entity's state changes, in real time.")
            .field(Field::new("entity_id", "Entity", FieldType::Entity).required())
            .field(
                Field::new("from", "From", FieldType::Text)
                    .placeholder("any")
                    .help("Only fire when the old state was this."),
            )
            .field(
                Field::new("to", "To", FieldType::Text)
                    .placeholder("any")
                    .help("Only fire when the new state is this."),
            )
            .output(event_fields(json!({
                "entity_id": string("Entity id"),
                "from": string("Old state"),
                "to": string("New state"),
                "from_state": { "type": "object", "description": "Old state object with attributes" },
                "to_state": { "type": "object", "description": "New state object with attributes" },
            })))
    }

    async fn run(&self, _: &Value, input: &Value, _: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        trigger_output(input)
    }
}

pub struct CronTrigger;

#[async_trait]
impl NodeExecutor for CronTrigger {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("trigger.cron", NodeKind::Trigger, "Schedule")
            .description("Fires on a cron schedule, in Home Assistant's time zone.")
            .field(
                Field::new("cron", "Cron", FieldType::Cron)
                    .required()
                    .default("0 7 * * 1-5")
                    .help("minute hour day-of-month month day-of-week"),
            )
            .output(event_fields(json!({
                "scheduled": string("The scheduled time, e.g. 2026-09-29 07:00"),
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        match config["cron"].as_str() {
            Some(expr) if !expr.trim().is_empty() => match Cron::from_str(expr) {
                Ok(_) => vec![],
                Err(e) => vec![format!("invalid cron expression: {e}")],
            },
            _ => vec![],
        }
    }

    async fn run(&self, _: &Value, input: &Value, _: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        trigger_output(input)
    }
}

pub struct ManualTrigger;

#[async_trait]
impl NodeExecutor for ManualTrigger {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("trigger.manual", NodeKind::Trigger, "Manual")
            .description("Fires from the Run button or the API.")
            .output(event_fields(json!({})))
    }

    async fn run(&self, _: &Value, input: &Value, _: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        trigger_output(input)
    }
}

pub struct MqttPublish {
    mqtt: Arc<dyn MqttPublisher>,
}

#[async_trait]
impl NodeExecutor for MqttPublish {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("mqtt.publish", NodeKind::Action, "MQTT publish")
            .description("Publishes a message to a topic.")
            .field(
                Field::new("topic", "Topic", FieldType::Template)
                    .required()
                    .placeholder("home/heater/set"),
            )
            .field(
                Field::new("payload", "Payload", FieldType::Template)
                    .placeholder("{{ trigger.to }}")
                    .help("Text or a template. JSON objects from earlier nodes are serialized."),
            )
            .field(
                Field::new(
                    "qos",
                    "QoS",
                    FieldType::Select {
                        options: vec!["0".into(), "1".into(), "2".into()],
                    },
                )
                .default("0"),
            )
            .field(Field::new("retain", "Retain", FieldType::Bool).default(false))
            .output(object(json!({
                "topic": string("The topic it was sent to"),
                "payload": string("The message that was sent"),
                "qos": { "type": "integer", "description": "Delivery level" },
                "retain": { "type": "boolean", "description": "Kept as the topic's last message" },
            })))
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        let topic = cfg["topic"].as_str().unwrap_or_default().trim();
        if topic.is_empty() {
            return Err(NodeError::new(ErrorKind::Config, "topic is empty"));
        }
        let payload = match &cfg["payload"] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let qos = match &cfg["qos"] {
            Value::String(s) => s.parse().unwrap_or(0),
            v => v.as_u64().unwrap_or(0) as u8,
        }
        .min(2);
        let retain = cfg["retain"].as_bool().unwrap_or(false);

        self.mqtt
            .publish(topic, payload.clone().into_bytes(), qos, retain)
            .await
            .map_err(|e| NodeError::new(ErrorKind::Mqtt, e).details(json!({ "topic": topic })))?;
        ctx.log(format!(
            "published {} bytes to {topic} (qos {qos}{})",
            payload.len(),
            if retain { ", retained" } else { "" }
        ));
        Ok(NodeOutput::out(json!({
            "topic": topic,
            "payload": payload,
            "qos": qos,
            "retain": retain,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeMqtt(Mutex<Vec<(String, String, u8, bool)>>);

    #[async_trait]
    impl MqttPublisher for FakeMqtt {
        async fn publish(&self, t: &str, p: Vec<u8>, q: u8, r: bool) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .push((t.into(), String::from_utf8(p).unwrap(), q, r));
            Ok(())
        }
    }

    #[tokio::test]
    async fn mqtt_publish_sends_rendered_config() {
        let fake = Arc::new(FakeMqtt::default());
        let node = MqttPublish { mqtt: fake.clone() };
        let cfg = json!({ "topic": "ergo/test", "payload": "on", "qos": "1", "retain": true });
        let (trigger, steps) = (json!({}), json!({}));
        let ctx = RunCtx::new(&trigger, &steps);
        let out = node.run(&cfg, &json!({}), &ctx).await.unwrap();
        assert_eq!(out.port, "out");
        assert_eq!(
            ctx.take_logs(),
            vec!["published 2 bytes to ergo/test (qos 1, retained)"]
        );
        assert_eq!(
            fake.0.lock().unwrap()[0],
            ("ergo/test".into(), "on".into(), 1, true)
        );
    }

    #[test]
    fn cron_validation() {
        assert!(
            CronTrigger
                .validate(&json!({ "cron": "0 7 * * 1-5" }))
                .is_empty()
        );
        assert_eq!(CronTrigger.validate(&json!({ "cron": "nope" })).len(), 1);
    }
}
