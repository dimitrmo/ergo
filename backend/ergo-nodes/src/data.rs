//! `data.filter` and `data.map`: pick, keep and reshape JSON.

use async_trait::async_trait;
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Map, Value, json};
use serde_json_path::JsonPath;

use crate::object;

fn compile(path: &str) -> Result<JsonPath, NodeError> {
    JsonPath::parse(path).map_err(|e| {
        NodeError::new(
            ErrorKind::Config,
            format!("JSONPath `{path}` is not valid: {e}"),
        )
    })
}

/// The array a path points at. A single matched array is used as is; a single
/// object becomes a one-item list (XML with one `<item>` looks like that);
/// several matches are the list.
fn select_items(input: &Value, path: &str) -> Result<Vec<Value>, NodeError> {
    let path = if path.trim().is_empty() {
        "$"
    } else {
        path.trim()
    };
    let matches = compile(path)?.query(input).all();
    Ok(match matches.as_slice() {
        [] => vec![],
        [Value::Array(items)] => items.clone(),
        [Value::Null] => vec![],
        [one] => vec![(*one).clone()],
        many => many.iter().map(|v| (*v).clone()).collect(),
    })
}

/// A field of an item: `$.a.b` as JSONPath, or a plain `a.b` key path.
fn field_of(item: &Value, field: &str) -> Result<Option<Value>, NodeError> {
    let field = field.trim();
    if field.starts_with('$') {
        let found = compile(field)?.query(item).all();
        return Ok(match found.as_slice() {
            [] => None,
            [one] => Some((*one).clone()),
            many => Some(Value::Array(many.iter().map(|v| (*v).clone()).collect())),
        });
    }
    Ok(field.split('.').try_fold(item, |v, k| v.get(k)).cloned())
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Template results count as true unless empty, false, 0, none or null.
fn truthy(text: &str) -> bool {
    !matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "" | "false" | "0" | "none" | "null"
    )
}

const OPS: &[&str] = &[
    "equals",
    "not_equals",
    "contains",
    "not_contains",
    "starts_with",
    "ends_with",
    "greater_than",
    "less_than",
    "exists",
    "not_exists",
];

fn rule_matches(op: &str, actual: Option<&Value>, expected: &str) -> bool {
    let text = actual.map(as_text).unwrap_or_default();
    let (a, b) = (text.to_lowercase(), expected.to_lowercase());
    match op {
        "exists" => actual.is_some_and(|v| !v.is_null()),
        "not_exists" => actual.is_none_or(Value::is_null),
        "equals" => match (
            actual.and_then(as_number),
            expected.trim().parse::<f64>().ok(),
        ) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
        "not_equals" => !rule_matches("equals", actual, expected),
        // A list field matches when any of its entries does.
        "contains" => match actual {
            Some(Value::Array(xs)) => xs.iter().any(|x| as_text(x).to_lowercase() == b),
            _ => a.contains(&b),
        },
        "not_contains" => !rule_matches("contains", actual, expected),
        "starts_with" => a.starts_with(&b),
        "ends_with" => a.ends_with(&b),
        "greater_than" => {
            matches!((actual.and_then(as_number), expected.trim().parse::<f64>()), (Some(x), Ok(y)) if x > y)
        }
        "less_than" => {
            matches!((actual.and_then(as_number), expected.trim().parse::<f64>()), (Some(x), Ok(y)) if x < y)
        }
        _ => false,
    }
}

/// Keeps the items of an array that match rules, a template or a JSONata test.
pub struct DataFilter;

#[async_trait]
impl NodeExecutor for DataFilter {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("data.filter", NodeKind::Data, "Filter")
            .description("Keeps the items of a list that match.")
            .field(
                Field::new("path", "List", FieldType::Text)
                    .default("$")
                    .placeholder("$.rss.channel.item")
                    .help("JSONPath to the list in the input."),
            )
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
                    .placeholder("{{ item.category == 'rust' }}"),
            )
            .field(
                Field::new("jsonata", "JSONata", FieldType::Text)
                    .raw()
                    .placeholder("category = 'rust' and $contains(title, 'async')")
                    .help("Runs on each item; the item is kept when the result is true."),
            )
            .ports(vec!["out", "empty"])
            .output(object(json!({
                "items": { "type": "array", "description": "The items that matched" },
                "count": { "type": "integer", "description": "How many matched" },
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut problems = vec![];
        if let Some(p) = config["path"].as_str().filter(|p| !p.trim().is_empty())
            && let Err(e) = compile(p)
        {
            problems.push(e.message);
        }
        if config["mode"].as_str() == Some("jsonata") {
            match config["jsonata"]
                .as_str()
                .map(str::trim)
                .filter(|e| !e.is_empty())
            {
                None => problems.push("Write a JSONata expression".into()),
                Some(e) => problems.extend(crate::jsonata::check(e).err()),
            }
        } else if config["mode"].as_str() == Some("expression") {
            if config["expression"]
                .as_str()
                .is_none_or(|e| e.trim().is_empty())
            {
                problems.push("Expression is required".into());
            }
        } else {
            let rules = config["rules"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            if rules.is_empty() {
                problems.push("Add at least one rule".into());
            }
            for r in rules {
                if r["field"].as_str().is_none_or(|f| f.trim().is_empty()) {
                    problems.push("Every rule needs a field".into());
                    break;
                }
                if !OPS.contains(&r["op"].as_str().unwrap_or_default()) {
                    problems.push(format!(
                        "Unknown rule operator `{}`",
                        r["op"].as_str().unwrap_or_default()
                    ));
                    break;
                }
            }
        }
        problems
    }

    async fn run(
        &self,
        cfg: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError> {
        let items = select_items(input, cfg["path"].as_str().unwrap_or("$"))?;
        let total = items.len();
        let jsonata = match cfg["mode"].as_str() {
            Some("jsonata") => Some(crate::jsonata::Jsonata::parse(
                cfg["jsonata"].as_str().unwrap_or_default(),
            )?),
            _ => None,
        };
        let mut kept = Vec::new();
        for item in items {
            let keep = if let Some(test) = &jsonata {
                // The item is the context; the whole input is `$input`.
                test.test(&item, ctx, &[("input", input)])?
            } else if cfg["mode"].as_str() == Some("expression") {
                let expr = cfg["expression"].as_str().unwrap_or_default();
                truthy(&ctx.render(expr, input, json!({ "item": item }))?)
            } else {
                let rules = cfg["rules"].as_array().cloned().unwrap_or_default();
                let mut results = Vec::with_capacity(rules.len());
                for r in &rules {
                    let actual = field_of(&item, r["field"].as_str().unwrap_or_default())?;
                    let expected = ctx.render(
                        r["value"].as_str().unwrap_or_default(),
                        input,
                        json!({ "item": item }),
                    )?;
                    results.push(rule_matches(
                        r["op"].as_str().unwrap_or("equals"),
                        actual.as_ref(),
                        &expected,
                    ));
                }
                if cfg["match"].as_str() == Some("any") {
                    results.iter().any(|b| *b)
                } else {
                    results.iter().all(|b| *b)
                }
            };
            if keep {
                kept.push(item);
            }
        }
        let count = kept.len();
        ctx.log(format!("kept {count} of {total} items"));
        let port = if count == 0 { "empty" } else { "out" };
        Ok(NodeOutput::port(
            port,
            json!({ "items": kept, "count": count }),
        ))
    }
}

/// Sets `a.b.c` in a JSON object, creating the nested objects on the way.
fn set_nested(target: &mut Map<String, Value>, name: &str, value: Value) {
    let mut parts = name
        .split('.')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .peekable();
    let mut obj = target;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            obj.insert(part.to_string(), value);
            return;
        }
        let entry = obj
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        obj = entry.as_object_mut().expect("just made it an object");
    }
}

/// Reshapes the input: each output field is a JSONPath or a template.
pub struct DataMap;

impl DataMap {
    fn map_one(
        &self,
        item: &Value,
        fields: &[Value],
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<Value, NodeError> {
        let mut out = Map::new();
        for f in fields {
            let name = f["name"].as_str().unwrap_or_default();
            if name.trim().is_empty() {
                continue;
            }
            let spec = f["value"].as_str().unwrap_or_default().trim();
            let value = if spec.starts_with('$') {
                field_of(item, spec)?.unwrap_or(Value::Null)
            } else {
                Value::String(ctx.render(spec, input, json!({ "item": item }))?)
            };
            set_nested(&mut out, name, value);
        }
        Ok(Value::Object(out))
    }
}

#[async_trait]
impl NodeExecutor for DataMap {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("data.map", NodeKind::Data, "Map")
            .description("Reshapes data into the fields you need.")
            .field(
                Field::new("mode", "How", FieldType::Select { options: vec!["fields".into(), "jsonata".into()] })
                    .default("fields")
                    .help("Fields: pick values one by one. JSONata: one expression for the whole result."),
            )
            .field(
                Field::new("path", "From", FieldType::Text)
                    .default("$")
                    .placeholder("$.items")
                    .help("JSONPath to what to reshape. A list is reshaped item by item."),
            )
            .field(Field::new("fields", "Fields", FieldType::Text).raw())
            .field(
                Field::new("expression", "JSONata", FieldType::Text)
                    .raw()
                    .placeholder("rss.channel.item.{ \"title\": title, \"link\": link }"),
            )
            .ports(vec!["out"])
            .output(object(json!({
                "items": { "type": "array", "description": "The reshaped items (when the input was a list)" },
                "count": { "type": "integer", "description": "How many items" },
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        if config["mode"].as_str() == Some("jsonata") {
            return match config["expression"]
                .as_str()
                .map(str::trim)
                .filter(|e| !e.is_empty())
            {
                None => vec!["Write a JSONata expression".into()],
                Some(e) => crate::jsonata::check(e).err().into_iter().collect(),
            };
        }
        let mut problems = vec![];
        if let Some(p) = config["path"].as_str().filter(|p| !p.trim().is_empty())
            && let Err(e) = compile(p)
        {
            problems.push(e.message);
        }
        let fields = config["fields"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        if fields
            .iter()
            .all(|f| f["name"].as_str().is_none_or(|n| n.trim().is_empty()))
        {
            problems.push("Add at least one field".into());
        }
        for f in fields {
            if let Some(v) = f["value"]
                .as_str()
                .map(str::trim)
                .filter(|v| v.starts_with('$'))
                && let Err(e) = compile(v)
            {
                problems.push(e.message);
            }
        }
        problems
    }

    async fn run(
        &self,
        cfg: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError> {
        if cfg["mode"].as_str() == Some("jsonata") {
            let expr = cfg["expression"].as_str().unwrap_or_default();
            // A list comes out as { count, items }, like the fields mode, so
            // later steps read it the same way whichever mode made it.
            let output = match crate::jsonata::eval(expr, input, ctx)? {
                None => {
                    ctx.log("the expression matched nothing");
                    json!({ "count": 0, "items": [] })
                }
                Some(Value::Array(items)) => {
                    ctx.log(format!("JSONata produced {} items", items.len()));
                    json!({ "count": items.len(), "items": items })
                }
                Some(one) => {
                    ctx.log("JSONata produced one value");
                    one
                }
            };
            return Ok(NodeOutput::out(output));
        }
        let fields = cfg["fields"].as_array().cloned().unwrap_or_default();
        let path = cfg["path"]
            .as_str()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or("$");
        let matches = compile(path)?.query(input).all();
        let output = match matches.as_slice() {
            [Value::Array(items)] => {
                let mapped = items
                    .iter()
                    .map(|i| self.map_one(i, &fields, input, ctx))
                    .collect::<Result<Vec<_>, _>>()?;
                ctx.log(format!(
                    "reshaped {} items into {} fields each",
                    mapped.len(),
                    fields.len()
                ));
                json!({ "count": mapped.len(), "items": mapped })
            }
            [one] => {
                ctx.log(format!("reshaped into {} fields", fields.len()));
                self.map_one(one, &fields, input, ctx)?
            }
            [] => {
                return Err(NodeError::new(
                    ErrorKind::Config,
                    format!("nothing in the input at `{path}`"),
                ));
            }
            many => {
                let mapped = many
                    .iter()
                    .map(|i| self.map_one(i, &fields, input, ctx))
                    .collect::<Result<Vec<_>, _>>()?;
                json!({ "count": mapped.len(), "items": mapped })
            }
        };
        Ok(NodeOutput::out(output))
    }
}
