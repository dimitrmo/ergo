//! `ha.action`: calls a Home Assistant action (a service), e.g. `light.turn_on`.
//! `ha.notify`: sends a notification through a `notify.*` action.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset};
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Value, json};

use crate::{object, string};

/// One action call, as the node hands it to Home Assistant.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionCall {
    pub domain: String,
    pub service: String,
    /// Entities to act on; empty for actions without a target.
    pub entity_ids: Vec<String>,
    pub data: Value,
    /// Ask for the action's answer (e.g. `weather.get_forecasts`).
    pub return_response: bool,
    /// The workflow making the call, so its own state changes can be told apart.
    pub workflow_id: String,
}

/// Calls HA actions and reads its state; implemented by the binary's
/// WebSocket connection.
#[async_trait]
pub trait HaCaller: Send + Sync {
    /// Returns HA's result: `{ "context": …, "response": … }`.
    async fn call_action(&self, call: ActionCall) -> Result<Value, String>;

    /// An entity's current state object (`state`, `attributes`, …), if HA
    /// has it.
    fn state(&self, entity_id: &str) -> Option<Value>;

    /// The time now in Home Assistant's time zone.
    fn local_now(&self) -> DateTime<FixedOffset>;
}

pub struct HaAction {
    pub(crate) ha: Arc<dyn HaCaller>,
}

impl HaAction {
    pub fn new(ha: Arc<dyn HaCaller>) -> Self {
        Self { ha }
    }
}

/// `light.turn_on` -> ("light", "turn_on").
fn split_action(action: &str) -> Option<(&str, &str)> {
    let (domain, service) = action.trim().split_once('.')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    (ok(domain) && ok(service)).then_some((domain, service))
}

/// The entity field: one id, or several separated by commas or spaces.
fn entity_ids(value: &Value) -> Vec<String> {
    match value {
        Value::Array(xs) => xs
            .iter()
            .filter_map(|x| x.as_str())
            .map(str::to_string)
            .collect(),
        Value::String(s) => s
            .split([',', ' ', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => vec![],
    }
}

/// The data field: a JSON object (after templates), or nothing.
fn action_data(value: &Value) -> Result<Value, NodeError> {
    match value {
        Value::Null => Ok(json!({})),
        Value::Object(_) => Ok(value.clone()),
        Value::String(s) if s.trim().is_empty() => Ok(json!({})),
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(v @ Value::Object(_)) => Ok(v),
            Ok(_) => Err(NodeError::new(
                ErrorKind::Config,
                "the data must be a JSON object, like {\"brightness_pct\": 40}",
            )),
            Err(e) => Err(NodeError::new(
                ErrorKind::Config,
                format!("the data isn't valid JSON: {e}"),
            )
            .details(json!({ "data": s }))),
        },
        _ => Err(NodeError::new(
            ErrorKind::Config,
            "the data must be a JSON object",
        )),
    }
}

#[async_trait]
impl NodeExecutor for HaAction {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("ha.action", NodeKind::Action, "HA action")
            .description("Calls a Home Assistant action, e.g. turn on a light or send a notification.")
            .field(
                Field::new("action", "Action", FieldType::Text)
                    .required()
                    .placeholder("light.turn_on")
                    .help("domain.action, as in Developer tools → Actions."),
            )
            .field(
                Field::new("entity_id", "Entities", FieldType::Template)
                    .placeholder("light.living_room")
                    .help("What to act on. Separate several with commas; leave empty if the action needs none."),
            )
            .field(
                Field::new("data", "Data", FieldType::Template)
                    .placeholder("{\"brightness_pct\": 40}")
                    .help("Options as a JSON object. Templates work, e.g. {{ input.level }}."),
            )
            .field(
                Field::new("response", "Wait for its answer", FieldType::Bool)
                    .default(false)
                    .help("For actions that return data, like weather.get_forecasts."),
            )
            .output(object(json!({
                "action": string("The action that was called"),
                "entity_id": { "type": "array", "description": "The entities it acted on" },
                "data": { "type": "object", "description": "The data it was called with" },
                "response": { "type": "object", "description": "The action's answer, if asked for" },
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut problems = vec![];
        if let Some(a) = config["action"]
            .as_str()
            .filter(|a| !a.trim().is_empty() && !a.contains("{{"))
            && split_action(a).is_none()
        {
            problems.push(format!(
                "`{}` isn't an action; use domain.action, like light.turn_on",
                a.trim()
            ));
        }
        // Data with templates is checked when it runs, once they're filled in.
        if let Some(d) = config["data"]
            .as_str()
            .filter(|d| !d.contains("{{") && !d.contains("{%"))
            && let Err(e) = action_data(&Value::String(d.to_string()))
        {
            problems.push(e.message);
        }
        problems
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        let action = cfg["action"].as_str().unwrap_or_default().trim();
        let (domain, service) = split_action(action).ok_or_else(|| {
            NodeError::new(
                ErrorKind::Config,
                format!("`{action}` isn't an action; use domain.action"),
            )
        })?;
        let entities = entity_ids(&cfg["entity_id"]);
        let data = action_data(&cfg["data"])?;
        let return_response = cfg["response"].as_bool().unwrap_or(false);

        let result = self
            .ha
            .call_action(ActionCall {
                domain: domain.into(),
                service: service.into(),
                entity_ids: entities.clone(),
                data: data.clone(),
                return_response,
                workflow_id: ctx.workflow_id.to_string(),
            })
            .await
            .map_err(|e| {
                NodeError::new(ErrorKind::Ha, e)
                    .details(json!({ "action": action, "entity_id": entities, "data": data }))
            })?;
        ctx.log(match entities.as_slice() {
            [] => format!("called {action}"),
            ids => format!("called {action} on {}", ids.join(", ")),
        });
        Ok(NodeOutput::out(json!({
            "action": action,
            "entity_id": entities,
            "data": data,
            "response": result.get("response").cloned().unwrap_or(Value::Null),
        })))
    }
}

pub struct HaNotify {
    ha: Arc<dyn HaCaller>,
}

impl HaNotify {
    pub fn new(ha: Arc<dyn HaCaller>) -> Self {
        Self { ha }
    }
}

/// `notify.mobile_app_pixel` -> "mobile_app_pixel"; anything else is refused.
fn notify_service(action: &str) -> Option<&str> {
    match split_action(action)? {
        ("notify", service) => Some(service),
        _ => None,
    }
}

#[async_trait]
impl NodeExecutor for HaNotify {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("ha.notify", NodeKind::Action, "Notify")
            .description("Sends a notification to a phone or another notifier.")
            .field(
                Field::new("service", "Send to", FieldType::Text)
                    .required()
                    .placeholder("notify.mobile_app_my_phone")
                    .help("A notify action, as in Developer tools → Actions."),
            )
            .field(Field::new("title", "Title", FieldType::Template).placeholder("Garage"))
            .field(
                Field::new("message", "Message", FieldType::Template)
                    .required()
                    .placeholder("The garage door is still open"),
            )
            .output(object(json!({
                "service": string("The notify action that was called"),
                "title": string("The title that was sent"),
                "message": string("The message that was sent"),
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        match config["service"].as_str().map(str::trim) {
            Some(a) if !a.is_empty() && !a.contains("{{") && notify_service(a).is_none() => {
                vec![format!(
                    "`{a}` isn't a notify action; pick one like notify.mobile_app_my_phone"
                )]
            }
            _ => vec![],
        }
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        let action = cfg["service"].as_str().unwrap_or_default().trim();
        let service = notify_service(action).ok_or_else(|| {
            NodeError::new(
                ErrorKind::Config,
                format!("`{action}` isn't a notify action"),
            )
        })?;
        let text = |key: &str| match &cfg[key] {
            Value::Null => String::new(),
            Value::String(s) => s.trim().to_string(),
            other => other.to_string(),
        };
        let (title, message) = (text("title"), text("message"));
        if message.is_empty() {
            return Err(NodeError::new(ErrorKind::Config, "the message is empty"));
        }
        let mut data = json!({ "message": message });
        if !title.is_empty() {
            data["title"] = json!(title);
        }
        self.ha
            .call_action(ActionCall {
                domain: "notify".into(),
                service: service.into(),
                entity_ids: vec![],
                data: data.clone(),
                return_response: false,
                workflow_id: ctx.workflow_id.to_string(),
            })
            .await
            .map_err(|e| {
                NodeError::new(ErrorKind::Ha, e).details(json!({ "action": action, "data": data }))
            })?;
        ctx.log(format!("sent a notification with {action}"));
        Ok(NodeOutput::out(json!({
            "service": action,
            "title": title,
            "message": message,
        })))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Records action calls; states and the clock are set by each test.
    #[derive(Default)]
    pub(crate) struct FakeHa {
        pub calls: Mutex<Vec<ActionCall>>,
        pub states: Mutex<HashMap<String, Value>>,
        pub now: Mutex<Option<DateTime<FixedOffset>>>,
    }

    impl FakeHa {
        pub fn set_state(&self, entity_id: &str, state: &str) {
            self.states.lock().unwrap().insert(
                entity_id.into(),
                json!({ "entity_id": entity_id, "state": state }),
            );
        }

        /// Sets the clock, e.g. "2026-10-05T22:30:00+03:00" (a Monday).
        pub fn set_now(&self, rfc3339: &str) {
            *self.now.lock().unwrap() = Some(DateTime::parse_from_rfc3339(rfc3339).unwrap());
        }
    }

    #[async_trait]
    impl HaCaller for FakeHa {
        async fn call_action(&self, call: ActionCall) -> Result<Value, String> {
            let answer = if call.return_response {
                json!({ "temp": 21 })
            } else {
                Value::Null
            };
            self.calls.lock().unwrap().push(call);
            Ok(json!({ "context": { "id": "c1" }, "response": answer }))
        }

        fn state(&self, entity_id: &str) -> Option<Value> {
            self.states.lock().unwrap().get(entity_id).cloned()
        }

        fn local_now(&self) -> DateTime<FixedOffset> {
            self.now
                .lock()
                .unwrap()
                .unwrap_or_else(|| chrono::Utc::now().fixed_offset())
        }
    }

    #[tokio::test]
    async fn notify_sends_title_and_message() {
        let fake = Arc::new(FakeHa::default());
        let node = HaNotify::new(fake.clone());
        let cfg = json!({ "service": "notify.mobile_app_pixel", "title": "Garage", "message": "Still open" });
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s).with_workflow("wf1");
        let out = node.run(&cfg, &json!({}), &ctx).await.unwrap();
        assert_eq!(out.output["message"], "Still open");
        let call = fake.calls.lock().unwrap()[0].clone();
        assert_eq!(
            (call.domain.as_str(), call.service.as_str()),
            ("notify", "mobile_app_pixel")
        );
        assert_eq!(
            call.data,
            json!({ "title": "Garage", "message": "Still open" })
        );
        // No title: only the message is sent.
        let cfg = json!({ "service": "notify.notify", "message": "Hi" });
        node.run(&cfg, &json!({}), &ctx).await.unwrap();
        assert_eq!(
            fake.calls.lock().unwrap()[1].data,
            json!({ "message": "Hi" })
        );
    }

    #[tokio::test]
    async fn notify_refuses_other_actions_and_empty_messages() {
        let node = HaNotify::new(Arc::new(FakeHa::default()));
        assert_eq!(
            node.validate(&json!({ "service": "light.turn_on" })).len(),
            1
        );
        assert!(
            node.validate(&json!({ "service": "notify.notify" }))
                .is_empty()
        );
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let e = node
            .run(
                &json!({ "service": "notify.notify", "message": " " }),
                &json!({}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Config);
    }

    #[tokio::test]
    async fn calls_the_action_with_targets_and_data() {
        let fake = Arc::new(FakeHa::default());
        let node = HaAction::new(fake.clone());
        let cfg = json!({
            "action": "light.turn_on",
            "entity_id": "light.a, light.b",
            "data": "{\"brightness_pct\": 40}",
            "response": true,
        });
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s).with_workflow("wf1");
        let out = node.run(&cfg, &json!({}), &ctx).await.unwrap();
        assert_eq!(out.output["response"], json!({ "temp": 21 }));
        assert_eq!(
            ctx.take_logs(),
            vec!["called light.turn_on on light.a, light.b"]
        );
        let call = fake.calls.lock().unwrap()[0].clone();
        assert_eq!(
            call,
            ActionCall {
                domain: "light".into(),
                service: "turn_on".into(),
                entity_ids: vec!["light.a".into(), "light.b".into()],
                data: json!({ "brightness_pct": 40 }),
                return_response: true,
                workflow_id: "wf1".into(),
            }
        );
    }

    #[test]
    fn validates_action_and_data() {
        let node = HaAction::new(Arc::new(FakeHa::default()));
        assert!(
            node.validate(&json!({ "action": "light.turn_on" }))
                .is_empty()
        );
        assert_eq!(
            node.validate(&json!({ "action": "turn on the light" }))
                .len(),
            1
        );
        assert_eq!(
            node.validate(&json!({ "action": "a.b", "data": "{nope" }))
                .len(),
            1
        );
        assert_eq!(
            node.validate(&json!({ "action": "a.b", "data": "[1]" }))
                .len(),
            1
        );
        // Templated data is only checked once rendered.
        assert!(
            node.validate(&json!({ "action": "a.b", "data": "{\"x\": {{ input.x }}}" }))
                .is_empty()
        );
    }
}
