//! Renders MiniJinja templates inside node config against the run context.

use minijinja::{Environment, UndefinedBehavior};
use serde_json::Value;

fn env() -> Environment<'static> {
    let mut env = Environment::new();
    // Lenient: a missing field renders as "" (a workflow with several
    // triggers can use `{{ trigger.to }}` even when a schedule fired), but a
    // misspelled top-level name such as `{{ tigger.to }}` still fails the node.
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    // Caps template work, so a runaway loop can't stall a run.
    env.set_fuel(Some(50_000));
    env
}

fn is_template(s: &str) -> bool {
    s.contains("{{") || s.contains("{%")
}

pub fn render_str(template: &str, ctx: &Value) -> Result<String, String> {
    if !is_template(template) {
        return Ok(template.to_string());
    }
    env()
        .render_str(template, ctx)
        .map_err(|e| format!("template error in `{template}`: {e}"))
}

/// Renders the config except the top-level keys in `raw`, which the node
/// renders itself.
pub fn render_config_except(config: &Value, ctx: &Value, raw: &[&str]) -> Result<Value, String> {
    match config {
        Value::Object(map) if !raw.is_empty() => Ok(Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let v = if raw.contains(&k.as_str()) {
                        v.clone()
                    } else {
                        render_config(v, ctx)?
                    };
                    Ok((k.clone(), v))
                })
                .collect::<Result<_, String>>()?,
        )),
        _ => render_config(config, ctx),
    }
}

/// Renders every string in `config` that contains a template.
pub fn render_config(config: &Value, ctx: &Value) -> Result<Value, String> {
    Ok(match config {
        Value::String(s) => Value::String(render_str(s, ctx)?),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| render_config(v, ctx))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), render_config(v, ctx)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_nested_strings() {
        let ctx = json!({ "trigger": { "to": "on" }, "n2": { "state": "3.4" } });
        let cfg =
            json!({ "topic": "ergo/{{ trigger.to }}", "payload": "t={{ n2.state }}", "qos": 1 });
        let out = render_config(&cfg, &ctx).unwrap();
        assert_eq!(
            out,
            json!({ "topic": "ergo/on", "payload": "t=3.4", "qos": 1 })
        );
    }

    #[test]
    fn plain_strings_pass_through() {
        assert_eq!(render_str("heater/on", &json!({})).unwrap(), "heater/on");
    }

    #[test]
    fn misspelled_names_are_errors() {
        assert!(render_str("{{ nope.x }}", &json!({})).is_err());
    }

    #[test]
    fn missing_fields_render_empty() {
        let ctx = json!({ "trigger": { "kind": "manual" } });
        assert_eq!(render_str("to={{ trigger.to }}", &ctx).unwrap(), "to=");
    }
}
