//! JSONata (https://jsonata.org) for reshaping data, via `jsonata-core`.
//!
//! The step's input is the context (`items.{ "title": title }`); the run's
//! trigger and earlier steps are bound as `$trigger` and `$steps`.

use ergo_core::{ErrorKind, NodeError, RunCtx};
use jsonata_core::evaluator::{Context, Evaluator, EvaluatorOptions};
use jsonata_core::parser;
use jsonata_core::value::JValue;
use serde_json::{Map, Value, json};

/// Guardrails: an expression is data shaping, not a program.
fn options() -> EvaluatorOptions {
    EvaluatorOptions {
        timeout_ms: Some(2_000),
        max_stack_depth: Some(200),
        max_sequence_length: Some(100_000),
    }
}

/// A syntax check for the editor.
pub(crate) fn check(expr: &str) -> Result<(), String> {
    parser::parse(expr)
        .map(|_| ())
        .map_err(|e| format!("JSONata: {e}"))
}

/// A parsed expression, run once per value (e.g. per item when filtering).
pub(crate) struct Jsonata<'e> {
    expr: &'e str,
    ast: jsonata_core::ast::AstNode,
}

impl<'e> Jsonata<'e> {
    pub(crate) fn parse(expr: &'e str) -> Result<Self, NodeError> {
        let ast = parser::parse(expr).map_err(|e| {
            NodeError::new(ErrorKind::Config, format!("JSONata: {e}"))
                .details(json!({ "expression": expr }))
        })?;
        Ok(Self { expr, ast })
    }

    /// Evaluates against `data`, with `$trigger`, `$steps` and any `extra`
    /// variables bound. `None` when it matched nothing.
    pub(crate) fn eval(
        &self,
        data: &Value,
        ctx: &RunCtx<'_>,
        extra: &[(&str, &Value)],
    ) -> Result<Option<Value>, NodeError> {
        Ok(self.eval_raw(data, ctx, extra)?.map(|v| to_json(&v)))
    }

    /// JSONata truthiness (`$boolean`): empty strings, 0, empty lists and
    /// nothing at all are false.
    pub(crate) fn test(
        &self,
        data: &Value,
        ctx: &RunCtx<'_>,
        extra: &[(&str, &Value)],
    ) -> Result<bool, NodeError> {
        Ok(self.eval_raw(data, ctx, extra)?.is_some_and(|v| truthy(&v)))
    }

    fn eval_raw(
        &self,
        data: &Value,
        ctx: &RunCtx<'_>,
        extra: &[(&str, &Value)],
    ) -> Result<Option<JValue>, NodeError> {
        let mut context = Context::new();
        context.bind("trigger".into(), JValue::from(ctx.trigger.clone()));
        context.bind("steps".into(), JValue::from(ctx.steps.clone()));
        for (name, value) in extra {
            context.bind((*name).into(), JValue::from((*value).clone()));
        }
        let result = Evaluator::with_options(context, options())
            .evaluate(&self.ast, &JValue::from(data.clone()))
            .map_err(|e| {
                NodeError::new(ErrorKind::Template, format!("JSONata: {}", e.message()))
                    .details(json!({ "expression": self.expr, "code": e.code() }))
            })?;
        Ok((!matches!(result, JValue::Undefined)).then_some(result))
    }
}

/// Evaluates `expr` against `input`. `None` when it matched nothing.
pub(crate) fn eval(
    expr: &str,
    input: &Value,
    ctx: &RunCtx<'_>,
) -> Result<Option<Value>, NodeError> {
    Jsonata::parse(expr)?.eval(input, ctx, &[])
}

fn truthy(v: &JValue) -> bool {
    match v {
        JValue::Null | JValue::Undefined => false,
        JValue::Bool(b) => *b,
        JValue::Number(n) => *n != 0.0,
        JValue::String(s) => !s.is_empty(),
        JValue::Array(items) => items.iter().any(truthy),
        JValue::Object(map) => !map.is_empty(),
        _ => false,
    }
}

/// Like the crate's own conversion, but whole numbers stay integers (20, not 20.0).
fn to_json(v: &JValue) -> Value {
    match v {
        JValue::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
            json!(*n as i64)
        }
        JValue::Array(items) => Value::Array(items.iter().map(to_json).collect()),
        JValue::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), to_json(v)))
                .collect::<Map<_, _>>(),
        ),
        other => Value::from(other),
    }
}
