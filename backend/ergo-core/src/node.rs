//! The node trait and the schema each node type declares.
//!
//! The editor draws palettes, config forms and ports from [`NodeSchema`], so a
//! new node type needs only a Rust implementation of [`NodeExecutor`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::model::DEFAULT_PORT;

/// How the engine and the editor treat a node type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// No input; starts runs.
    Trigger,
    /// Has side effects (publishes, calls, writes).
    Action,
    /// Pure data in, data out.
    Transform,
    /// Fetches and reshapes data: download, parse, filter, map. Reads only.
    Data,
    /// Controls the path or timing.
    Flow,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldType {
    Text,
    /// Text that may contain MiniJinja templates, e.g. `{{ trigger.to }}`.
    Template,
    /// An HA entity id, picked from live entities.
    Entity,
    Cron,
    Select {
        options: Vec<String>,
    },
    Bool,
    Number,
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    #[serde(flatten)]
    pub field_type: FieldType,
    pub required: bool,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub default: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<&'static str>,
    /// Templates in this field are left for the node to render itself (e.g.
    /// once per array item with `item`), not rendered by the engine first.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub raw: bool,
}

impl Field {
    pub fn new(key: &'static str, label: &'static str, field_type: FieldType) -> Self {
        Self {
            key,
            label,
            field_type,
            required: false,
            default: Value::Null,
            help: None,
            placeholder: None,
            raw: false,
        }
    }

    pub fn raw(mut self) -> Self {
        self.raw = true;
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn default(mut self, value: impl Into<Value>) -> Self {
        self.default = value.into();
        self
    }

    pub fn help(mut self, help: &'static str) -> Self {
        self.help = Some(help);
        self
    }

    pub fn placeholder(mut self, placeholder: &'static str) -> Self {
        self.placeholder = Some(placeholder);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeSchema {
    #[serde(rename = "type")]
    pub node_type: &'static str,
    pub kind: NodeKind,
    pub title: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
    pub ports: Vec<&'static str>,
    /// JSON Schema of what the node expects as `input`.
    pub input: Value,
    /// JSON Schema of the node's `output`; the editor offers its fields to
    /// later steps (e.g. as insert chips).
    pub output: Value,
}

impl NodeSchema {
    pub fn new(node_type: &'static str, kind: NodeKind, title: &'static str) -> Self {
        Self {
            node_type,
            kind,
            title,
            description: "",
            fields: Vec::new(),
            ports: vec![DEFAULT_PORT],
            input: serde_json::json!({}),
            output: serde_json::json!({}),
        }
    }

    pub fn input(mut self, schema: Value) -> Self {
        self.input = schema;
        self
    }

    pub fn output(mut self, schema: Value) -> Self {
        self.output = schema;
        self
    }

    pub fn description(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    pub fn ports(mut self, ports: Vec<&'static str>) -> Self {
        self.ports = ports;
        self
    }
}

/// What a node hands on: the port it leaves by and its JSON output, which
/// becomes the `input` of every node connected to that port.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub port: String,
    pub output: Value,
}

impl NodeOutput {
    pub fn out(output: Value) -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            output,
        }
    }

    pub fn port(port: &str, output: Value) -> Self {
        Self {
            port: port.to_string(),
            output,
        }
    }
}

/// What went wrong in a step, typed so the editor can explain it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Timeout,
    HttpStatus,
    Network,
    Parse,
    Template,
    Ha,
    Mqtt,
    Config,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeError {
    pub kind: ErrorKind,
    pub message: String,
    /// Extra facts, e.g. the HTTP status and the first 2 KB of the body.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl NodeError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: Value::Null,
        }
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Read-only view of the run for a node, plus a place to write log lines.
pub struct RunCtx<'a> {
    /// The event that started the run.
    pub trigger: &'a Value,
    /// Outputs of the steps that already ran on this path: `{ id: { output } }`.
    pub steps: &'a Value,
    /// The run's id; temp files are named after it so they're cleaned up.
    pub run_id: &'a str,
    /// The workflow running, e.g. so HA calls can be traced back to it.
    pub workflow_id: &'a str,
    /// Where nodes may put large temporary files (deleted when the run ends).
    pub temp_dir: Option<&'a std::path::Path>,
    logs: std::sync::Mutex<Vec<String>>,
}

impl<'a> RunCtx<'a> {
    pub fn new(trigger: &'a Value, steps: &'a Value) -> Self {
        Self {
            trigger,
            steps,
            run_id: "",
            workflow_id: "",
            temp_dir: None,
            logs: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn with_run(mut self, run_id: &'a str, temp_dir: Option<&'a std::path::Path>) -> Self {
        self.run_id = run_id;
        self.temp_dir = temp_dir;
        self
    }

    pub fn with_workflow(mut self, workflow_id: &'a str) -> Self {
        self.workflow_id = workflow_id;
        self
    }

    /// Adds a line to this step's log, shown in the step inspector.
    pub fn log(&self, line: impl Into<String>) {
        self.logs.lock().unwrap().push(line.into());
    }

    pub fn take_logs(&self) -> Vec<String> {
        std::mem::take(&mut self.logs.lock().unwrap())
    }

    /// Renders a template the way the engine does, with `input`, `trigger`
    /// and `steps`, plus `extra` (e.g. `{ "item": … }`) on top.
    pub fn render(&self, template: &str, input: &Value, extra: Value) -> Result<String, NodeError> {
        let mut ctx =
            serde_json::json!({ "input": input, "trigger": self.trigger, "steps": self.steps });
        if let (Some(c), Value::Object(e)) = (ctx.as_object_mut(), extra) {
            c.extend(e);
        }
        crate::template::render_str(template, &ctx)
            .map_err(|e| NodeError::new(ErrorKind::Template, e))
    }
}

#[async_trait]
pub trait NodeExecutor: Send + Sync {
    fn schema(&self) -> NodeSchema;

    /// Extra checks beyond required fields, e.g. a cron expression that parses.
    fn validate(&self, _config: &Value) -> Vec<String> {
        Vec::new()
    }

    /// How long a run of this node may take, when it isn't the engine's
    /// usual step timeout (e.g. a delay that waits for minutes on purpose).
    fn time_limit(&self, _config: &Value) -> Option<std::time::Duration> {
        None
    }

    /// Runs the node on `input` (the previous step's output). `config` has
    /// its templates already rendered.
    async fn run(
        &self,
        config: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError>;
}

/// All node types this build knows, by type name.
#[derive(Default, Clone)]
pub struct Registry {
    nodes: HashMap<&'static str, Arc<dyn NodeExecutor>>,
    /// Node types turned off by configuration, with why; workflows that
    /// still use them get this as their error.
    disabled: HashMap<&'static str, &'static str>,
}

impl Registry {
    pub fn register(&mut self, node: impl NodeExecutor + 'static) {
        let schema = node.schema();
        self.nodes.insert(schema.node_type, Arc::new(node));
    }

    /// Leaves `node_type` out, explaining it with `reason`.
    pub fn disable(&mut self, node_type: &'static str, reason: &'static str) {
        self.nodes.remove(node_type);
        self.disabled.insert(node_type, reason);
    }

    /// Why a node type can't be used: turned off, or unknown.
    pub fn unavailable(&self, node_type: &str) -> String {
        match self.disabled.get(node_type) {
            Some(reason) => (*reason).to_string(),
            None => format!("unknown node type `{node_type}`"),
        }
    }

    pub fn get(&self, node_type: &str) -> Option<&Arc<dyn NodeExecutor>> {
        self.nodes.get(node_type)
    }

    pub fn kind_of(&self, node_type: &str) -> Option<NodeKind> {
        self.get(node_type).map(|n| n.schema().kind)
    }

    /// Schemas sorted by kind, then title, for the palette.
    pub fn schemas(&self) -> Vec<NodeSchema> {
        let mut schemas: Vec<_> = self.nodes.values().map(|n| n.schema()).collect();
        schemas.sort_by_key(|s| (s.kind as u8, s.title));
        schemas
    }
}
