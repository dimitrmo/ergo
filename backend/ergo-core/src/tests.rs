//! Engine and validation tests with fake nodes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::engine::{MAX_STORED_BYTES, cap_for_storage};
use crate::*;

struct FakeTrigger;

#[async_trait]
impl NodeExecutor for FakeTrigger {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("test.trigger", NodeKind::Trigger, "Trigger")
    }
    async fn run(&self, _: &Value, input: &Value, _: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        Ok(NodeOutput::out(input.clone()))
    }
}

/// Outputs `{ msg }` from its rendered config; fails with a typed error when
/// `msg` is "boom"; waits `sleep_ms`; logs what it did.
struct Echo;

#[async_trait]
impl NodeExecutor for Echo {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("test.echo", NodeKind::Action, "Echo")
            .field(Field::new("msg", "Message", FieldType::Template).required())
    }
    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        if let Some(ms) = cfg["sleep_ms"].as_u64() {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        match cfg["msg"].as_str() {
            Some("boom") => {
                Err(NodeError::new(ErrorKind::Mqtt, "boom").details(json!({ "code": 7 })))
            }
            msg => {
                ctx.log(format!("echoed {msg:?}"));
                Ok(NodeOutput::out(json!({ "msg": msg })))
            }
        }
    }
}

#[derive(Default)]
struct Recorder {
    nodes: Mutex<Vec<NodeRecord>>,
    finished: Mutex<Vec<(RunStatus, Option<String>)>>,
    skipped: Mutex<Vec<String>>,
}

impl RunSink for Recorder {
    fn run_started(&self, _: &RunStart) {}
    fn node_finished(&self, node: &NodeRecord) {
        self.nodes.lock().unwrap().push(node.clone());
    }
    fn run_finished(&self, _: &str, status: RunStatus, error: Option<&str>) {
        self.finished
            .lock()
            .unwrap()
            .push((status, error.map(str::to_string)));
    }
    fn run_skipped(&self, _: &RunStart, reason: &str) {
        self.skipped.lock().unwrap().push(reason.to_string());
    }
}

fn registry() -> Arc<Registry> {
    let mut r = Registry::default();
    r.register(FakeTrigger);
    r.register(Echo);
    Arc::new(r)
}

fn graph(json: Value) -> Graph {
    serde_json::from_value(json).unwrap()
}

/// trigger -> a -> b, where a reads its input and b reads a's output two ways.
fn chain() -> Graph {
    graph(json!({
        "nodes": [
            { "id": "t", "type": "test.trigger" },
            { "id": "a", "type": "test.echo", "config": { "msg": "to={{ input.to }}" } },
            { "id": "b", "type": "test.echo", "config": { "msg": "{{ input.msg }} / {{ steps.a.output.msg }} / {{ trigger.to }}" } }
        ],
        "edges": [ { "from": "t", "to": "a" }, { "from": "a", "to": "b" } ]
    }))
}

async fn run(g: Graph, sink: Arc<Recorder>) -> StartOutcome {
    let engine = Engine::new(registry(), sink.clone(), 4, Duration::from_secs(1));
    let outcome = engine.start(RunRequest {
        workflow_id: "wf".into(),
        version: 1,
        graph: Arc::new(g),
        trigger_node: "t".into(),
        trigger: json!({ "to": "on" }),
    });
    for _ in 0..100 {
        if !sink.finished.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    outcome
}

#[tokio::test]
async fn each_step_gets_the_previous_output_as_input() {
    let sink = Arc::new(Recorder::default());
    run(chain(), sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    assert_eq!(nodes.len(), 3);
    // The trigger's output is the event; the first step gets it as input.
    assert_eq!(nodes[1].input, json!({ "to": "on" }));
    assert_eq!(nodes[1].config, json!({ "msg": "to=on" }));
    assert_eq!(nodes[1].output, json!({ "msg": "to=on" }));
    // The second step sees its input, an earlier step's output, and the trigger.
    assert_eq!(nodes[2].input, json!({ "msg": "to=on" }));
    assert_eq!(nodes[2].output, json!({ "msg": "to=on / to=on / on" }));
    assert_eq!(
        nodes[2].logs,
        vec![r#"echoed Some("to=on / to=on / on")"#.to_string()]
    );
    assert_eq!(nodes[2].attempts.len(), 1);
    assert_eq!(sink.finished.lock().unwrap()[0].0, RunStatus::Success);
}

#[tokio::test]
async fn a_step_reached_by_two_branches_runs_once_per_branch() {
    // t -> a, t -> b, a -> m, b -> m
    let g = graph(json!({
        "nodes": [
            { "id": "t", "type": "test.trigger" },
            { "id": "a", "type": "test.echo", "config": { "msg": "from a" } },
            { "id": "b", "type": "test.echo", "config": { "msg": "from b" } },
            { "id": "m", "type": "test.echo", "config": { "msg": "m got {{ input.msg }}" } }
        ],
        "edges": [
            { "from": "t", "to": "a" }, { "from": "t", "to": "b" },
            { "from": "a", "to": "m" }, { "from": "b", "to": "m" }
        ]
    }));
    let sink = Arc::new(Recorder::default());
    run(g, sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    let m: Vec<_> = nodes
        .iter()
        .filter(|n| n.node_id == "m")
        .map(|n| n.output["msg"].clone())
        .collect();
    assert_eq!(m, vec![json!("m got from a"), json!("m got from b")]);
}

#[tokio::test]
async fn steps_only_see_their_own_path() {
    // b runs on a different branch than a, so `steps.a` doesn't exist there:
    // reading from a step that didn't run on this path is a template error.
    let g = graph(json!({
        "nodes": [
            { "id": "t", "type": "test.trigger" },
            { "id": "a", "type": "test.echo", "config": { "msg": "a" } },
            { "id": "b", "type": "test.echo", "config": { "msg": "[{{ steps.a.output.msg }}]" } }
        ],
        "edges": [ { "from": "t", "to": "a" }, { "from": "t", "to": "b" } ]
    }));
    let sink = Arc::new(Recorder::default());
    run(g, sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    let b = nodes.iter().find(|n| n.node_id == "b").unwrap();
    assert_eq!(b.error.as_ref().unwrap().kind, ErrorKind::Template);
}

#[tokio::test]
async fn a_failing_step_records_a_typed_error_and_stops_the_run() {
    let mut g = chain();
    g.nodes[1].config = json!({ "msg": "boom" });
    let sink = Arc::new(Recorder::default());
    run(g, sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    assert_eq!(nodes.len(), 2);
    let err = nodes[1].error.as_ref().unwrap();
    assert_eq!(err.kind, ErrorKind::Mqtt);
    assert_eq!(err.details, json!({ "code": 7 }));
    assert!(nodes[1].port.is_none());
    let finished = sink.finished.lock().unwrap();
    assert_eq!(finished[0].0, RunStatus::Failed);
    assert_eq!(finished[0].1.as_deref(), Some("a test.echo: boom"));
}

#[tokio::test]
async fn slow_steps_time_out_with_a_timeout_error() {
    let mut g = chain();
    g.nodes[1].config = json!({ "msg": "x", "sleep_ms": 5000 });
    let sink = Arc::new(Recorder::default());
    run(g, sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    assert_eq!(nodes[1].error.as_ref().unwrap().kind, ErrorKind::Timeout);
}

#[tokio::test]
async fn template_errors_are_typed() {
    let mut g = chain();
    g.nodes[1].config = json!({ "msg": "{{ nope.x }}" });
    let sink = Arc::new(Recorder::default());
    run(g, sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    assert_eq!(nodes[1].error.as_ref().unwrap().kind, ErrorKind::Template);
}

#[tokio::test]
async fn single_mode_skips_overlapping_runs() {
    let mut g = chain();
    g.nodes[1].config = json!({ "msg": "x", "sleep_ms": 200 });
    let g = Arc::new(g);
    let sink = Arc::new(Recorder::default());
    let engine = Engine::new(registry(), sink.clone(), 4, Duration::from_secs(1));
    let req = || RunRequest {
        workflow_id: "wf".into(),
        version: 1,
        graph: g.clone(),
        trigger_node: "t".into(),
        trigger: json!({}),
    };
    assert!(matches!(engine.start(req()), StartOutcome::Started { .. }));
    assert!(matches!(engine.start(req()), StartOutcome::Skipped { .. }));
    assert_eq!(sink.skipped.lock().unwrap()[0], "already running");
}

#[test]
fn large_values_are_capped_for_storage() {
    let small = json!({ "a": 1 });
    assert_eq!(cap_for_storage(small.clone()), small);
    let big = json!({ "text": "x".repeat(MAX_STORED_BYTES + 10) });
    let capped = cap_for_storage(big.clone());
    assert_eq!(capped["truncated"], json!(true));
    assert_eq!(capped["size"], json!(big.to_string().len()));
    assert_eq!(capped["preview"].as_str().unwrap().len(), MAX_STORED_BYTES);
}

#[test]
fn validation_catches_structural_problems() {
    let g = graph(json!({
        "nodes": [
            { "id": "t", "type": "test.trigger" },
            { "id": "a", "type": "test.echo", "config": {} },
            { "id": "b", "type": "test.echo", "config": { "msg": "x" } },
            { "id": "c", "type": "nope" }
        ],
        "edges": [
            { "from": "t", "to": "a" },
            { "from": "a", "to": "b" },
            { "from": "b", "to": "a" },
            { "from": "a", "to": "t" }
        ]
    }));
    let issues = validate(&g, &registry());
    let text: Vec<String> = issues.iter().map(|i| i.message.clone()).collect();
    assert!(text.iter().any(|m| m == "Message is required"), "{text:?}");
    assert!(
        text.iter().any(|m| m.contains("unknown node type")),
        "{text:?}"
    );
    assert!(text.iter().any(|m| m.contains("loop")), "{text:?}");
    assert!(text.iter().any(|m| m.contains("incoming")), "{text:?}");
    assert!(has_errors(&issues));
}

#[test]
fn disabled_node_types_explain_themselves() {
    let mut r = (*registry()).clone();
    r.disable("test.echo", "echo is turned off");
    let g = graph(json!({
        "nodes": [
            { "id": "t", "type": "test.trigger", "config": {} },
            { "id": "a", "type": "test.echo", "config": { "message": "hi" } }
        ],
        "edges": [{ "from": "t", "to": "a" }]
    }));
    let issues = validate(&g, &r);
    assert!(r.get("test.echo").is_none());
    assert!(
        issues.iter().any(|i| i.message == "echo is turned off"),
        "{issues:?}"
    );
}

#[test]
fn a_valid_chain_has_no_errors() {
    assert!(!has_errors(&validate(&chain(), &registry())));
}

#[test]
fn unfinished_steps_block_going_live_but_not_a_test_run() {
    let mut g = chain();
    g.nodes[2].config = json!({});
    let issues = validate(&g, &registry());
    assert!(has_errors(&issues));
    assert!(!blocks_test_run(&issues));

    let mut broken = g.clone();
    broken
        .edges
        .push(serde_json::from_value(json!({ "from": "b", "to": "a" })).unwrap());
    assert!(blocks_test_run(&validate(&broken, &registry())));
}

#[tokio::test]
async fn an_unfinished_step_fails_without_running() {
    let mut g = chain();
    g.nodes[2].config = json!({});
    let sink = Arc::new(Recorder::default());
    run(g, sink.clone()).await;
    let nodes = sink.nodes.lock().unwrap();
    assert!(
        nodes[1].error.is_none(),
        "the finished step before it still runs"
    );
    let err = nodes[2].error.as_ref().unwrap();
    assert_eq!(err.kind, ErrorKind::Config);
    assert_eq!(err.message, "this step isn't finished: Message is required");
    assert!(nodes[2].logs.is_empty(), "it never ran");
}
