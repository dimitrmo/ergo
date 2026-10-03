//! Runs workflows: walks the graph from the trigger that fired. Each step
//! gets the previous step's output as its `input`, produces an `output`, and
//! the run follows the port it chose. A step reached by two branches runs
//! once per branch, each time with that branch's input.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tracing::{info, warn};
use uuid::Uuid;

use crate::model::Graph;
use crate::node::{ErrorKind, NodeError, NodeKind, Registry, RunCtx};
use crate::template::render_config_except;
use crate::validate::step_problems;

/// Stored step inputs and outputs are capped at this size.
pub const MAX_STORED_BYTES: usize = 256 * 1024;

/// A guard against runaway graphs: at most this many step executions per run.
pub const MAX_STEPS_PER_RUN: u32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Success,
    Failed,
    Skipped,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Success => "success",
            RunStatus::Failed => "failed",
            RunStatus::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunStart {
    pub id: String,
    pub workflow_id: String,
    /// The activated version that ran, or 0 for a test run of the draft.
    pub version: i64,
    pub trigger_node: String,
    pub trigger: Value,
    pub started_at: DateTime<Utc>,
}

/// One try of a step. Retries (M2) add more.
#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<NodeError>,
}

/// Everything recorded about one step of a run.
#[derive(Debug, Clone)]
pub struct NodeRecord {
    pub run_id: String,
    pub seq: u32,
    pub node_id: String,
    pub node_type: String,
    /// The port it left by (`out`, `true`, `empty`, `error`…); none if it failed.
    pub port: Option<String>,
    /// The JSON it received: the previous step's output, or the trigger event.
    pub input: Value,
    /// Its config after templates were rendered.
    pub config: Value,
    pub output: Value,
    pub error: Option<NodeError>,
    pub attempts: Vec<Attempt>,
    pub logs: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
}

/// Values over [`MAX_STORED_BYTES`] are cut and marked, keeping their size.
pub fn cap_for_storage(value: Value) -> Value {
    let text = value.to_string();
    if text.len() <= MAX_STORED_BYTES {
        return value;
    }
    let mut cut = MAX_STORED_BYTES;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    json!({ "truncated": true, "size": text.len(), "preview": &text[..cut] })
}

/// Where runs are recorded; the binary implements it with SQLite.
pub trait RunSink: Send + Sync {
    fn run_started(&self, run: &RunStart);
    fn node_finished(&self, node: &NodeRecord);
    fn run_finished(&self, run_id: &str, status: RunStatus, error: Option<&str>);
    fn run_skipped(&self, run: &RunStart, reason: &str);
}

pub struct RunRequest {
    pub workflow_id: String,
    pub version: i64,
    pub graph: Arc<Graph>,
    pub trigger_node: String,
    pub trigger: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StartOutcome {
    Started { run_id: String },
    Skipped { run_id: String, reason: String },
}

pub struct Engine {
    registry: Arc<Registry>,
    sink: Arc<dyn RunSink>,
    /// Workflows with a run in progress (`single` mode).
    running: Mutex<HashSet<String>>,
    slots: Arc<Semaphore>,
    node_timeout: Duration,
    /// Where nodes put large temporary files; a run's files go when it ends.
    temp_dir: Option<std::path::PathBuf>,
}

impl Engine {
    pub fn new(
        registry: Arc<Registry>,
        sink: Arc<dyn RunSink>,
        max_concurrent_runs: usize,
        node_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            sink,
            running: Mutex::new(HashSet::new()),
            slots: Arc::new(Semaphore::new(max_concurrent_runs)),
            node_timeout,
            temp_dir: None,
        })
    }

    /// Gives nodes a folder for large temporary files. Files are named
    /// `<run id>-…` and deleted when their run ends.
    pub fn with_temp_dir(self: Arc<Self>, dir: std::path::PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&dir);
        // Anything left from before a restart is stale.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let mut engine = Arc::try_unwrap(self)
            .unwrap_or_else(|_| panic!("with_temp_dir before sharing the engine"));
        engine.temp_dir = Some(dir);
        Arc::new(engine)
    }

    fn cleanup_temp(&self, run_id: &str) {
        let Some(dir) = &self.temp_dir else { return };
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with(run_id) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Starts a run in the background, or records why it was skipped.
    pub fn start(self: &Arc<Self>, req: RunRequest) -> StartOutcome {
        let run = RunStart {
            id: Uuid::new_v4().to_string(),
            workflow_id: req.workflow_id.clone(),
            version: req.version,
            trigger_node: req.trigger_node.clone(),
            trigger: req.trigger.clone(),
            started_at: Utc::now(),
        };
        // Test runs of a draft don't block (or get blocked by) the active version.
        let key = format!(
            "{}:{}",
            req.workflow_id,
            if req.version == 0 { "draft" } else { "active" }
        );

        if !self.running.lock().unwrap().insert(key.clone()) {
            return self.skip(&run, "already running");
        }
        let Ok(permit) = self.slots.clone().try_acquire_owned() else {
            self.running.lock().unwrap().remove(&key);
            return self.skip(&run, "too many runs in progress");
        };

        self.sink.run_started(&run);
        let run_id = run.id.clone();
        let engine = self.clone();
        tokio::spawn(async move {
            let (status, error) = engine.execute(&run, &req).await;
            engine.cleanup_temp(&run.id);
            engine.sink.run_finished(&run.id, status, error.as_deref());
            engine.running.lock().unwrap().remove(&key);
            drop(permit);
            info!(workflow = %run.workflow_id, run = %run.id, status = status.as_str(), "run finished");
        });
        StartOutcome::Started { run_id }
    }

    fn skip(&self, run: &RunStart, reason: &str) -> StartOutcome {
        info!(workflow = %run.workflow_id, reason, "run skipped");
        self.sink.run_skipped(run, reason);
        StartOutcome::Skipped {
            run_id: run.id.clone(),
            reason: reason.to_string(),
        }
    }

    async fn execute(&self, run: &RunStart, req: &RunRequest) -> (RunStatus, Option<String>) {
        let graph = &req.graph;
        // A branch: the node to run, its input, and the steps already run on this path.
        struct Branch {
            node_id: String,
            input: Value,
            steps: serde_json::Map<String, Value>,
        }
        let mut queue = VecDeque::from([Branch {
            node_id: req.trigger_node.clone(),
            input: req.trigger.clone(),
            steps: serde_json::Map::new(),
        }]);
        let mut seq = 0;

        while let Some(branch) = queue.pop_front() {
            if seq >= MAX_STEPS_PER_RUN {
                return (
                    RunStatus::Failed,
                    Some(format!("stopped after {MAX_STEPS_PER_RUN} steps")),
                );
            }
            let Some(node) = graph.node(&branch.node_id) else {
                return (
                    RunStatus::Failed,
                    Some(format!("missing node {}", branch.node_id)),
                );
            };
            let Some(exec) = self.registry.get(&node.kind) else {
                return (
                    RunStatus::Failed,
                    Some(self.registry.unavailable(&node.kind)),
                );
            };

            let steps = Value::Object(branch.steps.clone());
            let started_at = Utc::now();
            let clock = Instant::now();
            let schema = exec.schema();
            let is_trigger = schema.kind == NodeKind::Trigger;
            let raw: Vec<&str> = schema
                .fields
                .iter()
                .filter(|f| f.raw)
                .map(|f| f.key)
                .collect();
            let template_ctx =
                json!({ "input": branch.input, "trigger": req.trigger, "steps": steps });
            // An unfinished step (possible in a test run of the draft) fails
            // without running, rather than running with half its settings.
            let problems = step_problems(&node.config, exec.as_ref());
            let config = if !problems.is_empty() {
                Err(NodeError::new(
                    ErrorKind::Config,
                    format!("this step isn't finished: {}", problems.join(", ")),
                ))
            } else if is_trigger {
                Ok(node.config.clone())
            } else {
                render_config_except(&node.config, &template_ctx, &raw)
                    .map_err(|e| NodeError::new(ErrorKind::Template, e))
            };
            let ctx = RunCtx::new(&req.trigger, &steps)
                .with_run(&run.id, self.temp_dir.as_deref())
                .with_workflow(&run.workflow_id);
            let result = match &config {
                Ok(cfg) => {
                    let limit = exec.time_limit(cfg).unwrap_or(self.node_timeout);
                    match tokio::time::timeout(limit, exec.run(cfg, &branch.input, &ctx)).await {
                        Ok(r) => r,
                        Err(_) => Err(NodeError::new(
                            ErrorKind::Timeout,
                            format!("timed out after {} s", limit.as_secs()),
                        )),
                    }
                }
                Err(e) => Err(e.clone()),
            };
            let duration_ms = clock.elapsed().as_millis() as u64;

            seq += 1;
            let mut record = NodeRecord {
                run_id: run.id.clone(),
                seq,
                node_id: node.id.clone(),
                node_type: node.kind.clone(),
                port: None,
                input: cap_for_storage(branch.input.clone()),
                config: cap_for_storage(config.unwrap_or(Value::Null)),
                output: Value::Null,
                error: None,
                attempts: vec![Attempt {
                    started_at,
                    duration_ms,
                    error: result.as_ref().err().cloned(),
                }],
                logs: ctx.take_logs(),
                started_at,
                duration_ms,
            };

            match result {
                Ok(out) => {
                    record.port = Some(out.port.clone());
                    record.output = cap_for_storage(out.output.clone());
                    self.sink.node_finished(&record);
                    let mut next_steps = branch.steps;
                    next_steps.insert(node.id.clone(), json!({ "output": out.output }));
                    // One branch per outgoing edge on the chosen port.
                    for next in graph.next(&node.id, &out.port) {
                        queue.push_back(Branch {
                            node_id: next.to_string(),
                            input: out.output.clone(),
                            steps: next_steps.clone(),
                        });
                    }
                }
                Err(e) => {
                    // M1: every error stops the run. Per-node on_error lands in M2.
                    warn!(run = %run.id, node = %node.id, error = %e, "step failed");
                    record.error = Some(e.clone());
                    self.sink.node_finished(&record);
                    return (
                        RunStatus::Failed,
                        Some(format!("{} {}: {}", node.id, node.kind, e.message)),
                    );
                }
            }
        }
        (RunStatus::Success, None)
    }
}
