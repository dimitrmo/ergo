//! ergo's workflow model, validation, templating and engine.
//!
//! Nothing here talks to Home Assistant, MQTT or HTTP; node implementations
//! live in `ergo-nodes` and the wiring in the `ergo` binary.

pub mod engine;
pub mod model;
pub mod node;
pub mod template;
pub mod validate;

pub use engine::{Engine, NodeRecord, RunRequest, RunSink, RunStart, RunStatus, StartOutcome};
pub use model::{Edge, Graph, Mode, Node, Position};
pub use node::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema,
    Registry, RunCtx,
};
pub use validate::{Issue, Severity, has_errors, validate};

#[cfg(test)]
mod tests;
