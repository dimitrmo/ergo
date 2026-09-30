//! The workflow document: a graph of nodes and edges, stored as JSON.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped whenever the graph format changes; loaders migrate older documents.
pub const SCHEMA_VERSION: u32 = 1;

pub const DEFAULT_PORT: &str = "out";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// A trigger that fires while a run is in progress is skipped (and logged).
    #[default]
    Single,
}

/// The part of a workflow that the editor draws and the engine runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// Node type, e.g. `trigger.state` or `mqtt.publish`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Optional short name, e.g. `arrive`; exposed to templates as `trigger.id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub position: Position,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    #[serde(default = "default_port", rename = "fromPort")]
    pub from_port: String,
    pub to: String,
}

fn default_port() -> String {
    DEFAULT_PORT.to_string()
}

impl Graph {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Nodes connected to `from`'s `port`, in edge order.
    pub fn next(&self, from: &str, port: &str) -> impl Iterator<Item = &str> {
        self.edges
            .iter()
            .filter(move |e| e.from == from && e.from_port == port)
            .map(|e| e.to.as_str())
    }
}
