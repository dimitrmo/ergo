//! `ha.action`: calls a Home Assistant action (a service), e.g. `light.turn_on`.

use std::sync::Arc;

use async_trait::async_trait;
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

/// Calls HA actions; implemented by the binary's WebSocket connection.
#[async_trait]
pub trait HaCaller: Send + Sync {
    /// Returns HA's result: `{ "context": …, "response": … }`.
    async fn call_action(&self, call: ActionCall) -> Result<Value, String>;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub(crate) struct FakeHa(pub Mutex<Vec<ActionCall>>);

    #[async_trait]
    impl HaCaller for FakeHa {
        async fn call_action(&self, call: ActionCall) -> Result<Value, String> {
            let answer = if call.return_response {
                json!({ "temp": 21 })
            } else {
                Value::Null
            };
            self.0.lock().unwrap().push(call);
            Ok(json!({ "context": { "id": "c1" }, "response": answer }))
        }
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
        let call = fake.0.lock().unwrap()[0].clone();
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
