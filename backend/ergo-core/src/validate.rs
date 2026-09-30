//! Checks a graph before it can be activated.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::model::Graph;
use crate::node::{NodeKind, Registry};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Blocks activation.
    Error,
    /// Shown in the editor; the workflow can still be activated.
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub message: String,
}

impl Issue {
    fn error(node: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            node: node.map(str::to_string),
            message: message.into(),
        }
    }

    fn warning(node: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            node: node.map(str::to_string),
            message: message.into(),
        }
    }
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

fn is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        _ => false,
    }
}

pub fn validate(graph: &Graph, registry: &Registry) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut ids = HashSet::new();

    for node in &graph.nodes {
        if !ids.insert(node.id.as_str()) {
            issues.push(Issue::error(Some(&node.id), "duplicate node id"));
        }
        let Some(exec) = registry.get(&node.kind) else {
            issues.push(Issue::error(
                Some(&node.id),
                format!("unknown node type `{}`", node.kind),
            ));
            continue;
        };
        let schema = exec.schema();
        for field in schema.fields.iter().filter(|f| f.required) {
            if is_empty(node.config.get(field.key)) {
                issues.push(Issue::error(
                    Some(&node.id),
                    format!("{} is required", field.label),
                ));
            }
        }
        for message in exec.validate(&node.config) {
            issues.push(Issue::error(Some(&node.id), message));
        }
    }

    let kinds: HashMap<&str, Option<NodeKind>> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), registry.kind_of(&n.kind)))
        .collect();

    for edge in &graph.edges {
        let (Some(from), Some(_)) = (graph.node(&edge.from), graph.node(&edge.to)) else {
            issues.push(Issue::error(
                None,
                format!("edge {} -> {} points at a missing node", edge.from, edge.to),
            ));
            continue;
        };
        if let Some(exec) = registry.get(&from.kind)
            && !exec.schema().ports.contains(&edge.from_port.as_str())
        {
            issues.push(Issue::error(
                Some(&edge.from),
                format!("unknown port `{}`", edge.from_port),
            ));
        }
        if kinds.get(edge.to.as_str()).copied().flatten() == Some(NodeKind::Trigger) {
            issues.push(Issue::error(
                Some(&edge.to),
                "triggers cannot have incoming connections",
            ));
        }
    }

    if has_cycle(graph) {
        issues.push(Issue::error(None, "the graph contains a loop"));
    }

    let triggers: Vec<&str> = kinds
        .iter()
        .filter(|(_, k)| **k == Some(NodeKind::Trigger))
        .map(|(id, _)| *id)
        .collect();
    if triggers.is_empty() {
        issues.push(Issue::warning(
            None,
            "no trigger: this workflow can only run manually",
        ));
    }

    let reachable = reachable_from(graph, &triggers);
    for node in &graph.nodes {
        if !reachable.contains(node.id.as_str()) && !triggers.contains(&node.id.as_str()) {
            issues.push(Issue::warning(
                Some(&node.id),
                "no trigger reaches this node",
            ));
        }
    }

    issues
}

fn has_cycle(graph: &Graph) -> bool {
    // Kahn's algorithm: a cycle leaves nodes that never reach in-degree 0.
    let mut indegree: HashMap<&str, usize> =
        graph.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
    for e in &graph.edges {
        if let Some(d) = indegree.get_mut(e.to.as_str()) {
            *d += 1;
        }
    }
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut seen = 0;
    while let Some(id) = ready.pop() {
        seen += 1;
        for e in graph.edges.iter().filter(|e| e.from == id) {
            if let Some(d) = indegree.get_mut(e.to.as_str()) {
                *d -= 1;
                if *d == 0 {
                    ready.push(e.to.as_str());
                }
            }
        }
    }
    seen != indegree.len()
}

fn reachable_from<'a>(graph: &'a Graph, starts: &[&'a str]) -> HashSet<&'a str> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = starts.to_vec();
    while let Some(id) = stack.pop() {
        for e in graph.edges.iter().filter(|e| e.from == id) {
            if seen.insert(e.to.as_str()) {
                stack.push(e.to.as_str());
            }
        }
    }
    seen
}
