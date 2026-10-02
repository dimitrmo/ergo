//! `flow.if`: sends the run one way or the other.

use async_trait::async_trait;
use ergo_core::{
    Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Value, json};

use crate::data::{OPS, field_of, rule_matches, truthy};

/// Checks a condition on its input and leaves by `true` or `false`, passing
/// the input on unchanged, so the steps after it see what came before it.
pub struct FlowIf;

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
                for r in rules {
                    if r["field"].as_str().is_none_or(|f| f.trim().is_empty()) {
                        return vec!["Every rule needs a field".into()];
                    }
                    let op = r["op"].as_str().unwrap_or_default();
                    if !OPS.contains(&op) {
                        return vec![format!("Unknown rule operator `{op}`")];
                    }
                }
                vec![]
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn port(cfg: Value, input: Value) -> String {
        let (t, s) = (json!({ "to": "on" }), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let out = FlowIf.run(&cfg, &input, &ctx).await.unwrap();
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
            FlowIf.validate(&json!({ "mode": "rules" })),
            vec!["Add at least one rule"]
        );
        assert!(
            FlowIf
                .validate(&json!({ "rules": [{ "field": "to", "op": "equals", "value": "on" }] }))
                .is_empty()
        );
        assert_eq!(
            FlowIf
                .validate(&json!({ "mode": "jsonata", "jsonata": "((" }))
                .len(),
            1
        );
    }
}
