//! Flow steps: `flow.if` sends the run one way or the other, `flow.delay`
//! pauses it, and `flow.wait` holds it until an entity reaches a state.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{Datelike, NaiveTime, Timelike};
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Value, json};

use crate::HaCaller;
use crate::data::{OPS, field_of, rule_matches, truthy};

/// The longest a delay or a wait may be; longer pauses belong in a schedule.
const MAX_PAUSE: Duration = Duration::from_secs(24 * 3600);
/// Extra time the engine allows on top of a pause, for the steps' own work.
const PAUSE_SLACK: Duration = Duration::from_secs(10);
/// How often a wait looks at the entity's state (read from ergo's cache).
const WAIT_POLL: Duration = Duration::from_millis(500);
/// When the sun is down, before this hour counts as "before sunrise" and
/// after it as "after sunset".
const NOON: u32 = 12;

/// Checks a condition on its input and leaves by `true` or `false`, passing
/// the input on unchanged, so the steps after it see what came before it.
/// Besides value rules, a rule can check the time, the day or the sun, in
/// Home Assistant's time zone.
pub struct FlowIf {
    ha: Arc<dyn HaCaller>,
}

impl FlowIf {
    pub fn new(ha: Arc<dyn HaCaller>) -> Self {
        Self { ha }
    }

    /// A time, day or sun rule: whether it holds and how to log it. `None`
    /// for a value rule.
    fn home_rule(&self, rule: &Value) -> Option<Result<(bool, String), NodeError>> {
        let now = self.ha.local_now();
        match rule["kind"].as_str() {
            Some("time") => {
                let t = now.time();
                let (after, before) = (hhmm(&rule["after"]), hhmm(&rule["before"]));
                let ok = match (after, before) {
                    // A window that crosses midnight, e.g. 22:00 to 07:00.
                    (Some(a), Some(b)) if a > b => t >= a || t < b,
                    (Some(a), Some(b)) => t >= a && t < b,
                    (Some(a), None) => t >= a,
                    (None, Some(b)) => t < b,
                    (None, None) => {
                        return Some(Err(config("a time rule needs a start or an end")));
                    }
                };
                Some(Ok((
                    ok,
                    format!("time {} in the window", now.format("%H:%M")),
                )))
            }
            Some("weekday") => {
                let today = now.weekday().num_days_from_sunday() as u64;
                let days = rule["days"].as_array().cloned().unwrap_or_default();
                let ok = days.iter().any(|d| d.as_u64() == Some(today));
                Some(Ok((
                    ok,
                    format!("today ({}) is a chosen day", now.format("%A")),
                )))
            }
            Some("sun") => {
                let Some(state) = self.ha.state("sun.sun") else {
                    return Some(Err(NodeError::new(
                        ErrorKind::Ha,
                        "sun.sun isn't available; turn on Home Assistant's Sun integration",
                    )));
                };
                let down = state["state"].as_str() == Some("below_horizon");
                let morning = now.hour() < NOON;
                let want = rule["sun"].as_str().unwrap_or("down");
                let ok = match want {
                    "up" => !down,
                    "after_sunset" => down && !morning,
                    "before_sunrise" => down && morning,
                    _ => down,
                };
                Some(Ok((
                    ok,
                    format!("sun {} ({want})", if down { "down" } else { "up" }),
                )))
            }
            _ => None,
        }
    }
}

fn config(message: impl Into<String>) -> NodeError {
    NodeError::new(ErrorKind::Config, message)
}

/// "07:30" -> 07:30; anything else -> None.
fn hhmm(value: &Value) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value.as_str()?.trim(), "%H:%M").ok()
}

/// Problems with one If rule, whatever its kind.
fn rule_problem(r: &Value) -> Option<String> {
    match r["kind"].as_str() {
        Some("time") => {
            let set = |k: &str| r[k].as_str().is_some_and(|s| !s.trim().is_empty());
            if !set("after") && !set("before") {
                return Some("A time rule needs a start or an end".into());
            }
            ["after", "before"]
                .into_iter()
                .find(|k| set(k) && hhmm(&r[*k]).is_none())
                .map(|k| {
                    format!(
                        "`{}` isn't a time like 07:30",
                        r[k].as_str().unwrap_or_default()
                    )
                })
        }
        Some("weekday") => r["days"]
            .as_array()
            .is_none_or(|d| d.is_empty())
            .then(|| "Pick at least one day".into()),
        Some("sun") => {
            let sun = r["sun"].as_str().unwrap_or("down");
            (!["up", "down", "after_sunset", "before_sunrise"].contains(&sun))
                .then(|| format!("Unknown sun setting `{sun}`"))
        }
        _ => {
            if r["field"].as_str().is_none_or(|f| f.trim().is_empty()) {
                return Some("Every rule needs a field".into());
            }
            let op = r["op"].as_str().unwrap_or_default();
            (!OPS.contains(&op)).then(|| format!("Unknown rule operator `{op}`"))
        }
    }
}

#[async_trait]
impl NodeExecutor for FlowIf {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("flow.if", NodeKind::Flow, "If")
            .description("Continues one way when a condition holds, another way when it doesn't.")
            .field(
                Field::new(
                    "mode",
                    "How",
                    FieldType::Select {
                        options: vec!["rules".into(), "expression".into(), "jsonata".into()],
                    },
                )
                .default("rules"),
            )
            .field(
                Field::new(
                    "match",
                    "Match",
                    FieldType::Select {
                        options: vec!["all".into(), "any".into()],
                    },
                )
                .default("all"),
            )
            .field(Field::new("rules", "Rules", FieldType::Text).raw())
            .field(
                Field::new("expression", "Expression", FieldType::Template)
                    .raw()
                    .placeholder("{{ input.to == 'on' and trigger.time > '18:00' }}"),
            )
            .field(
                Field::new("jsonata", "JSONata", FieldType::Text)
                    .raw()
                    .placeholder("to = 'on'"),
            )
            .ports(vec!["true", "false"])
            // The input passes through; the editor shows the steps before it.
            .output(json!({}))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        match config["mode"].as_str() {
            Some("jsonata") => match config["jsonata"]
                .as_str()
                .map(str::trim)
                .filter(|e| !e.is_empty())
            {
                None => vec!["Write a JSONata expression".into()],
                Some(e) => crate::jsonata::check(e).err().into_iter().collect(),
            },
            Some("expression") => {
                if config["expression"]
                    .as_str()
                    .is_none_or(|e| e.trim().is_empty())
                {
                    vec!["Expression is required".into()]
                } else {
                    vec![]
                }
            }
            _ => {
                let rules = config["rules"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                if rules.is_empty() {
                    return vec!["Add at least one rule".into()];
                }
                rules.iter().find_map(rule_problem).into_iter().collect()
            }
        }
    }

    async fn run(
        &self,
        cfg: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError> {
        let holds = match cfg["mode"].as_str() {
            Some("jsonata") => crate::jsonata::Jsonata::parse(
                cfg["jsonata"].as_str().unwrap_or_default(),
            )?
            .test(input, ctx, &[("input", input)])?,
            Some("expression") => truthy(&ctx.render(
                cfg["expression"].as_str().unwrap_or_default(),
                input,
                json!({}),
            )?),
            _ => {
                let rules = cfg["rules"].as_array().cloned().unwrap_or_default();
                let mut results = Vec::with_capacity(rules.len());
                for r in &rules {
                    if let Some(home) = self.home_rule(r) {
                        let (ok, what) = home?;
                        ctx.log(format!("{what}: {}", if ok { "yes" } else { "no" }));
                        results.push(ok);
                        continue;
                    }
                    let field = r["field"].as_str().unwrap_or_default();
                    // A field is a path in the input, or a template such as
                    // {{ trigger.to }} for anything else.
                    let actual = if field.contains("{{") {
                        Some(Value::String(ctx.render(field, input, json!({}))?))
                    } else {
                        field_of(input, field)?
                    };
                    let expected =
                        ctx.render(r["value"].as_str().unwrap_or_default(), input, json!({}))?;
                    let op = r["op"].as_str().unwrap_or("equals");
                    let ok = rule_matches(op, actual.as_ref(), &expected);
                    ctx.log(format!(
                        "{field} ({}) {op} {expected}: {}",
                        actual.as_ref().map_or("not set".to_string(), |v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        }),
                        if ok { "yes" } else { "no" }
                    ));
                    results.push(ok);
                }
                if cfg["match"].as_str() == Some("any") {
                    results.iter().any(|b| *b)
                } else {
                    results.iter().all(|b| *b)
                }
            }
        };
        ctx.log(if holds {
            "the condition holds"
        } else {
            "the condition doesn't hold"
        });
        Ok(NodeOutput::port(
            if holds { "true" } else { "false" },
            input.clone(),
        ))
    }
}

const UNITS: [&str; 3] = ["seconds", "minutes", "hours"];

fn unit_field(default: &'static str) -> Field {
    Field::new(
        "unit",
        "Unit",
        FieldType::Select {
            options: UNITS.iter().map(|u| u.to_string()).collect(),
        },
    )
    .default(default)
}

/// `amount` of `unit` from a config, at most a day.
fn pause(config: &Value, amount_key: &str) -> Result<Duration, String> {
    let amount = match &config[amount_key] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|a| a.is_finite() && *a > 0.0)
    .ok_or("Set how long, as a number above 0")?;
    let seconds = match config["unit"].as_str().unwrap_or("minutes") {
        "seconds" => amount,
        "hours" => amount * 3600.0,
        _ => amount * 60.0,
    };
    let d = Duration::from_secs_f64(seconds);
    if d > MAX_PAUSE {
        return Err("A pause can be at most 24 hours".into());
    }
    Ok(d)
}

/// Templated amounts are only known when the step runs.
fn templated(config: &Value, key: &str) -> bool {
    config[key].as_str().is_some_and(|s| s.contains("{{"))
}

/// "90 s", "5 min", "1 h 30 min".
fn human(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{} s", d.as_secs_f64()),
        60..3600 if s.is_multiple_of(60) => format!("{} min", s / 60),
        60..3600 => format!("{} min {} s", s / 60, s % 60),
        _ if s.is_multiple_of(3600) => format!("{} h", s / 3600),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// Pauses the run, then carries on with the same data.
pub struct FlowDelay;

#[async_trait]
impl NodeExecutor for FlowDelay {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("flow.delay", NodeKind::Flow, "Delay")
            .description("Waits a while before the next step.")
            .field(
                Field::new("amount", "For", FieldType::Number)
                    .required()
                    .default(5),
            )
            .field(unit_field("minutes"))
            // The input passes through, like If.
            .output(json!({}))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        if templated(config, "amount") {
            return vec![];
        }
        pause(config, "amount").err().into_iter().collect()
    }

    fn time_limit(&self, config: &Value) -> Option<Duration> {
        pause(config, "amount").ok().map(|d| d + PAUSE_SLACK)
    }

    async fn run(
        &self,
        cfg: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError> {
        let d = pause(cfg, "amount").map_err(config)?;
        tokio::time::sleep(d).await;
        ctx.log(format!("waited {}", human(d)));
        Ok(NodeOutput::out(input.clone()))
    }
}

/// Holds the run until an entity reaches a state, then leaves by `out`; if
/// it doesn't within the time limit, leaves by `timeout`. Either way the
/// data passes through.
pub struct FlowWait {
    ha: Arc<dyn HaCaller>,
}

impl FlowWait {
    pub fn new(ha: Arc<dyn HaCaller>) -> Self {
        Self { ha }
    }
}

#[async_trait]
impl NodeExecutor for FlowWait {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("flow.wait", NodeKind::Flow, "Wait until")
            .description("Waits until something reaches a state, or gives up after a while.")
            .field(Field::new("entity_id", "Entity", FieldType::Entity).required())
            .field(
                Field::new("state", "State", FieldType::Text)
                    .required()
                    .placeholder("off"),
            )
            .field(
                Field::new("timeout", "Give up after", FieldType::Number)
                    .required()
                    .default(30),
            )
            .field(unit_field("minutes"))
            .ports(vec!["out", "timeout"])
            .output(json!({}))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        if templated(config, "timeout") {
            return vec![];
        }
        pause(config, "timeout").err().into_iter().collect()
    }

    fn time_limit(&self, config: &Value) -> Option<Duration> {
        pause(config, "timeout").ok().map(|d| d + PAUSE_SLACK)
    }

    async fn run(
        &self,
        cfg: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError> {
        let entity = cfg["entity_id"].as_str().unwrap_or_default().trim();
        let want = cfg["state"].as_str().unwrap_or_default().trim();
        if entity.is_empty() || want.is_empty() {
            return Err(config("pick an entity and the state to wait for"));
        }
        let limit = pause(cfg, "timeout").map_err(config)?;
        let started = tokio::time::Instant::now();
        loop {
            let now = self
                .ha
                .state(entity)
                .and_then(|s| s["state"].as_str().map(str::to_string));
            if now.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(want)) {
                ctx.log(format!(
                    "{entity} is {want} after {}",
                    human(Duration::from_secs(started.elapsed().as_secs()))
                ));
                return Ok(NodeOutput::out(input.clone()));
            }
            if started.elapsed() >= limit {
                ctx.log(format!(
                    "{entity} is still {} after {}; giving up",
                    now.as_deref().unwrap_or("unknown"),
                    human(limit)
                ));
                return Ok(NodeOutput::port("timeout", input.clone()));
            }
            tokio::time::sleep(WAIT_POLL.min(limit)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ha::tests::FakeHa;

    fn flow_if() -> FlowIf {
        FlowIf::new(Arc::new(FakeHa::default()))
    }

    async fn port(cfg: Value, input: Value) -> String {
        let (t, s) = (json!({ "to": "on" }), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let out = flow_if().run(&cfg, &input, &ctx).await.unwrap();
        assert_eq!(out.output, input, "the input passes through");
        out.port
    }

    #[tokio::test]
    async fn rules_pick_the_way() {
        let input = json!({ "to": "on", "to_state": { "attributes": { "temperature": 23.5 } } });
        let rule = |field: &str, op: &str, value: &str| json!({ "field": field, "op": op, "value": value });
        let cfg = |rules: Value, m: &str| json!({ "mode": "rules", "match": m, "rules": rules });
        assert_eq!(
            port(
                cfg(json!([rule("to", "equals", "ON")]), "all"),
                input.clone()
            )
            .await,
            "true"
        );
        assert_eq!(
            port(
                cfg(
                    json!([rule(
                        "to_state.attributes.temperature",
                        "greater_than",
                        "25"
                    )]),
                    "all"
                ),
                input.clone()
            )
            .await,
            "false"
        );
        let both = json!([
            rule("to", "equals", "on"),
            rule("to_state.attributes.temperature", "greater_than", "25")
        ]);
        assert_eq!(port(cfg(both.clone(), "all"), input.clone()).await, "false");
        assert_eq!(port(cfg(both, "any"), input.clone()).await, "true");
        // A template field reads anything, here the trigger.
        assert_eq!(
            port(
                cfg(json!([rule("{{ trigger.to }}", "equals", "on")]), "all"),
                json!({})
            )
            .await,
            "true"
        );
        assert_eq!(
            port(cfg(json!([rule("missing", "exists", "")]), "all"), input).await,
            "false"
        );
    }

    #[tokio::test]
    async fn expressions_and_jsonata() {
        let input = json!({ "level": 7 });
        assert_eq!(
            port(
                json!({ "mode": "expression", "expression": "{{ input.level > 5 }}" }),
                input.clone()
            )
            .await,
            "true"
        );
        assert_eq!(
            port(json!({ "mode": "jsonata", "jsonata": "level < 5" }), input).await,
            "false"
        );
    }

    #[test]
    fn validation() {
        assert_eq!(
            flow_if().validate(&json!({ "mode": "rules" })),
            vec!["Add at least one rule"]
        );
        assert!(
            flow_if()
                .validate(&json!({ "rules": [{ "field": "to", "op": "equals", "value": "on" }] }))
                .is_empty()
        );
        assert_eq!(
            flow_if()
                .validate(&json!({ "mode": "jsonata", "jsonata": "((" }))
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn time_day_and_sun_rules() {
        let ha = Arc::new(FakeHa::default());
        let node = FlowIf::new(ha.clone());
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let check = |rule: Value| {
            let cfg = json!({ "mode": "rules", "rules": [rule] });
            let node = &node;
            let ctx = &ctx;
            async move { node.run(&cfg, &json!({}), ctx).await.map(|o| o.port) }
        };
        // Monday 22:30.
        ha.set_now("2026-10-05T22:30:00+03:00");
        ha.set_state("sun.sun", "below_horizon");
        let night = json!({ "kind": "time", "after": "22:00", "before": "07:00" });
        assert_eq!(check(night.clone()).await.unwrap(), "true");
        assert_eq!(
            check(json!({ "kind": "time", "after": "08:00", "before": "18:00" }))
                .await
                .unwrap(),
            "false"
        );
        assert_eq!(
            check(json!({ "kind": "time", "before": "23:00" }))
                .await
                .unwrap(),
            "true"
        );
        assert_eq!(
            check(json!({ "kind": "weekday", "days": [1, 2, 3, 4, 5] }))
                .await
                .unwrap(),
            "true"
        );
        assert_eq!(
            check(json!({ "kind": "weekday", "days": [0, 6] }))
                .await
                .unwrap(),
            "false"
        );
        assert_eq!(
            check(json!({ "kind": "sun", "sun": "after_sunset" }))
                .await
                .unwrap(),
            "true"
        );
        assert_eq!(
            check(json!({ "kind": "sun", "sun": "before_sunrise" }))
                .await
                .unwrap(),
            "false"
        );
        assert_eq!(
            check(json!({ "kind": "sun", "sun": "up" })).await.unwrap(),
            "false"
        );
        // Tuesday 05:00: still night, the wrap-around window holds.
        ha.set_now("2026-10-06T05:00:00+03:00");
        assert_eq!(check(night).await.unwrap(), "true");
        assert_eq!(
            check(json!({ "kind": "sun", "sun": "before_sunrise" }))
                .await
                .unwrap(),
            "true"
        );
        // Without the Sun integration, a sun rule fails clearly.
        ha.states.lock().unwrap().clear();
        let e = check(json!({ "kind": "sun", "sun": "down" }))
            .await
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Ha);
    }

    #[test]
    fn home_rule_validation() {
        let v = |rule: Value| flow_if().validate(&json!({ "rules": [rule] }));
        assert!(v(json!({ "kind": "time", "after": "22:00" })).is_empty());
        assert_eq!(
            v(json!({ "kind": "time" })),
            vec!["A time rule needs a start or an end"]
        );
        assert_eq!(v(json!({ "kind": "time", "after": "25:99" })).len(), 1);
        assert_eq!(
            v(json!({ "kind": "weekday", "days": [] })),
            vec!["Pick at least one day"]
        );
        assert!(v(json!({ "kind": "sun", "sun": "after_sunset" })).is_empty());
    }

    #[tokio::test]
    async fn delay_waits_then_passes_the_input_on() {
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let cfg = json!({ "amount": "0.05", "unit": "seconds" });
        let clock = std::time::Instant::now();
        let out = FlowDelay.run(&cfg, &json!({ "a": 1 }), &ctx).await.unwrap();
        assert!(clock.elapsed() >= Duration::from_millis(50));
        assert_eq!((out.port.as_str(), out.output), ("out", json!({ "a": 1 })));
        assert_eq!(
            FlowDelay.time_limit(&json!({ "amount": 5, "unit": "minutes" })),
            Some(Duration::from_secs(310))
        );
        assert!(
            FlowDelay
                .validate(&json!({ "amount": 2, "unit": "hours" }))
                .is_empty()
        );
        assert_eq!(
            FlowDelay
                .validate(&json!({ "amount": 25, "unit": "hours" }))
                .len(),
            1
        );
        assert_eq!(FlowDelay.validate(&json!({ "amount": 0 })).len(), 1);
        assert_eq!(human(Duration::from_secs(5400)), "1 h 30 min");
    }

    #[tokio::test]
    async fn wait_until_a_state_or_time_out() {
        let ha = Arc::new(FakeHa::default());
        let node = FlowWait::new(ha.clone());
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let cfg = json!({ "entity_id": "cover.garage", "state": "closed", "timeout": "0.2", "unit": "seconds" });
        ha.set_state("cover.garage", "open");
        let out = node.run(&cfg, &json!({}), &ctx).await.unwrap();
        assert_eq!(out.port, "timeout");
        // Reached while waiting.
        let later = ha.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            later.set_state("cover.garage", "Closed");
        });
        let cfg = json!({ "entity_id": "cover.garage", "state": "closed", "timeout": 5, "unit": "seconds" });
        let out = node.run(&cfg, &json!({ "x": 1 }), &ctx).await.unwrap();
        assert_eq!((out.port.as_str(), out.output), ("out", json!({ "x": 1 })));
    }
}
