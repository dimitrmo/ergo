//! `text.compose`: builds a message from a template.

use async_trait::async_trait;
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Value, json};

use crate::{object, string};

pub struct TextCompose;

#[async_trait]
impl NodeExecutor for TextCompose {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("text.compose", NodeKind::Transform, "Compose")
            .description("Builds a message from earlier data.")
            .field(
                Field::new("template", "Message", FieldType::Template)
                    .required()
                    .placeholder("New post: {{ input.items[0].title }}"),
            )
            .field(
                Field::new(
                    "format",
                    "Format",
                    FieldType::Select { options: vec!["plain".into(), "markdown".into(), "json".into()] },
                )
                .default("plain"),
            )
            .output(object(json!({
                "text": string("The composed message"),
                "json": { "type": "object", "description": "The message as JSON (JSON format only)" },
            })))
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        // The engine has already rendered the template.
        let text = cfg["template"].as_str().unwrap_or_default().to_string();
        let format = cfg["format"].as_str().unwrap_or("plain");
        ctx.log(format!(
            "composed {} characters ({format})",
            text.chars().count()
        ));
        if format == "json" {
            let parsed: Value = serde_json::from_str(&text).map_err(|e| {
                NodeError::new(
                    ErrorKind::Parse,
                    format!("the message isn't valid JSON: {e}"),
                )
                .details(json!({ "line": e.line(), "column": e.column(), "text": text }))
            })?;
            return Ok(NodeOutput::out(json!({ "text": text, "json": parsed })));
        }
        Ok(NodeOutput::out(json!({ "text": text })))
    }
}
