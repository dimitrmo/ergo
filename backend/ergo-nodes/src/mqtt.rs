//! `trigger.mqtt`, and MQTT topic filter rules shared with the binary.

use async_trait::async_trait;
use ergo_core::{
    Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Value, json};

use crate::{event_fields, string, trigger_output};

/// Checks a subscription filter the way brokers do, for a clear error here.
pub fn valid_topic_filter(f: &str) -> Result<(), String> {
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

/// Whether `topic` matches the subscription `filter` (`+` one level, `#` the
/// rest). As the MQTT spec says, wildcards at the start don't match `$SYS/…`.
pub fn topic_matches(filter: &str, topic: &str) -> bool {
    if topic.starts_with('$') && (filter.starts_with('+') || filter.starts_with('#')) {
        return false;
    }
    let mut f = filter.split('/');
    let mut t = topic.split('/');
    loop {
        match (f.next(), t.next()) {
            (Some("#"), _) => return true,
            (Some("+"), Some(_)) => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// Whether every topic `inner` matches is also matched by `outer`, so
/// subscribing to `outer` alone is enough.
pub fn filter_covers(outer: &str, inner: &str) -> bool {
    let mut o = outer.split('/');
    let mut i = inner.split('/');
    loop {
        match (o.next(), i.next()) {
            (Some("#"), _) => return true,
            (Some("+"), Some(x)) if x != "#" => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// Starts a run when a message arrives on a topic. The binary subscribes for
/// active workflows and starts the runs; this node passes the event on.
pub struct MqttTrigger;

#[async_trait]
impl NodeExecutor for MqttTrigger {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("trigger.mqtt", NodeKind::Trigger, "MQTT message")
            .description("Fires when a message arrives on an MQTT topic.")
            .field(
                Field::new("topic", "Topic", FieldType::Text)
                    .required()
                    .placeholder("zigbee2mqtt/hall_button/action")
                    .help("A topic, or a filter: + matches one level, # the rest."),
            )
            .field(
                Field::new("payload", "Only when the message is", FieldType::Text)
                    .placeholder("any")
                    .help("Fire only for this exact message, e.g. single."),
            )
            .output(event_fields(json!({
                "topic": string("The topic it arrived on"),
                "payload": string("The message as text"),
                "json": { "type": "object", "description": "The message as JSON, if it is JSON" },
                "qos": { "type": "integer", "description": "Delivery level" },
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        match config["topic"].as_str().map(str::trim) {
            Some(t) if !t.is_empty() => valid_topic_filter(t).err().into_iter().collect(),
            _ => vec![],
        }
    }

    async fn run(&self, _: &Value, input: &Value, _: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        trigger_output(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_topic_filters() {
        assert!(valid_topic_filter("#").is_ok());
        assert!(valid_topic_filter("home/+/state").is_ok());
        assert!(valid_topic_filter("home/#").is_ok());
        assert!(valid_topic_filter("home/#/x").is_err());
        assert!(valid_topic_filter("home/a+").is_err());
        assert!(valid_topic_filter("").is_err());
    }

    #[test]
    fn matches_topics() {
        assert!(topic_matches("home/+/state", "home/hall/state"));
        assert!(!topic_matches("home/+/state", "home/hall/x/state"));
        assert!(topic_matches("home/#", "home"));
        assert!(topic_matches("home/#", "home/a/b"));
        assert!(topic_matches("#", "anything/at/all"));
        assert!(!topic_matches("#", "$SYS/broker/uptime"));
        assert!(topic_matches("$SYS/#", "$SYS/broker/uptime"));
        assert!(!topic_matches("home/a", "home/b"));
        assert!(!topic_matches("home/a", "home/a/b"));
    }

    #[test]
    fn knows_which_filters_cover_others() {
        assert!(filter_covers("#", "home/+/state"));
        assert!(filter_covers("home/#", "home/a"));
        assert!(filter_covers("home/+/state", "home/hall/state"));
        assert!(!filter_covers("home/+", "home/#"));
        assert!(!filter_covers("home/a/+", "home/+/b"));
        assert!(filter_covers("a/b", "a/b"));
    }
}
