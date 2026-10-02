//! REST API under `/api`, plus `/health` and `/ready`.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ergo_core::{Graph, NodeKind, RunRequest, blocks_test_run, has_errors, validate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{error, warn};

use crate::db::Db;
use crate::ha::Ha;
use crate::mqtt::Mqtt;
use crate::triggers::Triggers;
use ergo_core::Engine;

pub struct AppState {
    pub db: Arc<Db>,
    pub engine: Arc<Engine>,
    pub ha: Arc<Ha>,
    pub mqtt: Arc<Mqtt>,
    pub triggers: Arc<Triggers>,
    pub started: Instant,
    /// Run history limits: (days, max runs).
    pub retention: (u32, u32),
}

type AppStateRef = State<Arc<AppState>>;

pub struct ApiError(StatusCode, Value);

impl ApiError {
    fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, json!({ "error": "not found" }))
    }
    fn bad(status: StatusCode, message: impl Into<String>) -> Self {
        Self(status, json!({ "error": message.into() }))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        error!(error = %e, "request failed");
        Self::bad(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/api/workflows", get(list_workflows).post(create_workflow))
        .route(
            "/api/workflows/{id}",
            get(get_workflow).put(save_workflow).delete(delete_workflow),
        )
        .route("/api/workflows/{id}/activate", post(activate))
        .route("/api/workflows/{id}/enable", post(enable))
        .route("/api/workflows/{id}/disable", post(disable))
        .route("/api/workflows/{id}/run", post(run_workflow))
        .route("/api/export", get(export_workflows))
        .route("/api/import", post(import_workflows))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/nodes", get(nodes))
        .route("/api/entities", get(entities))
        .route("/api/cron/preview", post(cron_preview))
        .route("/api/db/tables", get(db_tables))
        .route("/api/db/tables/{name}", get(db_rows))
        .route("/api/mqtt/messages", get(mqtt_messages).delete(mqtt_clear))
        .route("/api/mqtt/watch", post(mqtt_watch))
        .with_state(state)
}

/// Liveness for the add-on watchdog. Deliberately ignores HA and MQTT, so an
/// HA restart doesn't get ergo restarted too.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready(State(s): AppStateRef) -> Response {
    let ha = s.ha.status();
    let mqtt = s.mqtt.status();
    let db = s.db.ping();
    // MQTT is optional: only a configured-but-down broker degrades readiness.
    let ok = ha.connected && db.is_ok() && (!mqtt.configured || mqtt.connected);
    let body = json!({
        "status": if ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_s": s.started.elapsed().as_secs(),
        "checks": {
            "ha_websocket": {
                "ok": ha.connected,
                "error": ha.error,
                "last_event_at": ha.last_event_at,
                "ha_version": ha.version,
            },
            "mqtt": {
                "ok": !mqtt.configured || mqtt.connected,
                "configured": mqtt.configured,
                "connected": mqtt.connected,
                "broker": mqtt.broker,
                "error": mqtt.error,
            },
            "database": { "ok": db.is_ok(), "error": db.err().map(|e| e.to_string()) },
            "scheduler": { "ok": true, "time_zone": s.triggers.time_zone().name() },
        }
    });
    let code = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body)).into_response()
}

fn workflow_json(s: &AppState, id: &str) -> ApiResult {
    let wf = s.db.get_workflow(id)?.ok_or_else(ApiError::not_found)?;
    let issues = validate(&wf.draft, s.engine.registry());
    Ok(Json(json!({ "workflow": wf, "issues": issues })))
}

async fn list_workflows(State(s): AppStateRef) -> ApiResult {
    Ok(Json(json!(s.db.list_workflows()?)))
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
    #[serde(default)]
    draft: Option<Graph>,
}

async fn create_workflow(State(s): AppStateRef, Json(body): Json<CreateBody>) -> ApiResult {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad(
            StatusCode::UNPROCESSABLE_ENTITY,
            "name is required",
        ));
    }
    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    s.db.create_workflow(&id, name, &body.draft.unwrap_or_default())?;
    workflow_json(&s, &id)
}

async fn get_workflow(State(s): AppStateRef, Path(id): Path<String>) -> ApiResult {
    workflow_json(&s, &id)
}

#[derive(Deserialize)]
struct SaveBody {
    name: Option<String>,
    draft: Option<Graph>,
}

async fn save_workflow(
    State(s): AppStateRef,
    Path(id): Path<String>,
    Json(body): Json<SaveBody>,
) -> ApiResult {
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if !s.db.save_draft(&id, name, body.draft.as_ref())? {
        return Err(ApiError::not_found());
    }
    workflow_json(&s, &id)
}

async fn delete_workflow(State(s): AppStateRef, Path(id): Path<String>) -> ApiResult {
    let wf = s.db.get_workflow(&id)?.ok_or_else(ApiError::not_found)?;
    // A workflow that is live and on must be stopped first, so nothing is
    // deleted while its triggers can still fire.
    if wf.enabled && wf.active_version.is_some() {
        return Err(ApiError::bad(
            StatusCode::CONFLICT,
            "Turn this workflow off before deleting it.",
        ));
    }
    if !s.db.delete_workflow(&id)? {
        return Err(ApiError::not_found());
    }
    s.triggers.reload()?;
    Ok(Json(json!({ "deleted": id })))
}

async fn activate(State(s): AppStateRef, Path(id): Path<String>) -> ApiResult {
    let wf = s.db.get_workflow(&id)?.ok_or_else(ApiError::not_found)?;
    let issues = validate(&wf.draft, s.engine.registry());
    if has_errors(&issues) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "error": "fix the errors before activating", "issues": issues }),
        ));
    }
    s.db.activate(&id, &wf.draft)?;
    s.triggers.reload()?;
    workflow_json(&s, &id)
}

async fn set_enabled(s: &AppState, id: &str, enabled: bool) -> ApiResult {
    if !s.db.set_enabled(id, enabled)? {
        return Err(ApiError::not_found());
    }
    s.triggers.reload()?;
    workflow_json(s, id)
}

async fn enable(State(s): AppStateRef, Path(id): Path<String>) -> ApiResult {
    set_enabled(&s, &id, true).await
}

async fn disable(State(s): AppStateRef, Path(id): Path<String>) -> ApiResult {
    set_enabled(&s, &id, false).await
}

#[derive(Deserialize, Default)]
struct RunBody {
    /// Trigger node to start from; default: the first manual trigger, else the first trigger.
    node: Option<String>,
    /// Run the draft (a test run) instead of the active version.
    #[serde(default)]
    draft: bool,
}

async fn run_workflow(
    State(s): AppStateRef,
    Path(id): Path<String>,
    body: Option<Json<RunBody>>,
) -> ApiResult {
    let body = body.map(|b| b.0).unwrap_or_default();
    let wf = s.db.get_workflow(&id)?.ok_or_else(ApiError::not_found)?;
    let (version, graph) = if body.draft {
        // Unfinished steps don't stop a test run; they fail if it reaches them.
        let issues = validate(&wf.draft, s.engine.registry());
        if blocks_test_run(&issues) {
            return Err(ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "error": "fix the errors before running", "issues": issues }),
            ));
        }
        (0, wf.draft)
    } else {
        s.db.active_graph(&id)?.ok_or_else(|| {
            ApiError::bad(
                StatusCode::CONFLICT,
                "this workflow has no active version yet",
            )
        })?
    };

    let registry = s.engine.registry();
    let is_trigger = |kind: &str| registry.kind_of(kind) == Some(NodeKind::Trigger);
    let node = match &body.node {
        Some(n) => graph.node(n),
        None => graph
            .nodes
            .iter()
            .find(|n| n.kind == "trigger.manual")
            .or_else(|| graph.nodes.iter().find(|n| is_trigger(&n.kind))),
    }
    .filter(|n| is_trigger(&n.kind))
    .cloned()
    .ok_or_else(|| ApiError::bad(StatusCode::UNPROCESSABLE_ENTITY, "no trigger to start from"))?;

    // A run started by hand simulates its trigger with current data. Every
    // trigger carries `time`, so templates can always use {{ trigger.time }}.
    let now = chrono::Utc::now().with_timezone(&s.triggers.time_zone());
    let time = now.format("%Y-%m-%d %H:%M").to_string();
    let trigger = match node.kind.as_str() {
        "trigger.state" => {
            let entity_id = node.config["entity_id"].as_str().unwrap_or_default();
            let current = s.ha.state(entity_id);
            json!({
                "kind": "state", "time": time, "node": node.id, "id": node.label, "simulated": true,
                "entity_id": entity_id,
                "from": null,
                "to": current.as_ref().and_then(|c| c["state"].as_str()),
                "from_state": null,
                "to_state": current,
            })
        }
        "trigger.cron" => {
            json!({
                "kind": "cron", "time": time, "node": node.id, "id": node.label, "simulated": true,
                "scheduled": now.format("%Y-%m-%d %H:%M").to_string(),
                "scheduled_iso": now.to_rfc3339(),
            })
        }
        _ => json!({ "kind": "manual", "time": time, "node": node.id, "id": node.label }),
    };

    let outcome = s.engine.start(RunRequest {
        workflow_id: id,
        version,
        graph: Arc::new(graph),
        trigger_node: node.id,
        trigger,
    });
    Ok(Json(json!(outcome)))
}

/// The export file: workflows' names and drafts, to import into another ergo.
#[derive(Serialize, Deserialize)]
pub struct ExportFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub exported_at: String,
    #[serde(default)]
    pub ergo_version: String,
    pub workflows: Vec<ExportedWorkflow>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportedWorkflow {
    pub name: String,
    pub draft: Graph,
}

const EXPORT_FORMAT: &str = "ergo.workflows";
const EXPORT_VERSION: u32 = 1;

impl ExportFile {
    pub fn new(workflows: Vec<ExportedWorkflow>) -> Self {
        Self {
            format: EXPORT_FORMAT.into(),
            version: EXPORT_VERSION,
            exported_at: chrono::Utc::now().to_rfc3339(),
            ergo_version: env!("CARGO_PKG_VERSION").into(),
            workflows,
        }
    }
}

#[derive(Deserialize)]
struct ExportQuery {
    /// Comma-separated workflow ids; all workflows when absent.
    ids: Option<String>,
}

async fn export_workflows(State(s): AppStateRef, Query(q): Query<ExportQuery>) -> ApiResult {
    let ids: Option<Vec<&str>> = q.ids.as_deref().map(|ids| ids.split(',').collect());
    let workflows =
        s.db.list_workflows()?
            .into_iter()
            .filter(|wf| ids.as_ref().is_none_or(|ids| ids.contains(&wf.id.as_str())))
            .map(|wf| ExportedWorkflow {
                name: wf.name,
                draft: wf.draft,
            })
            .collect();
    Ok(Json(json!(ExportFile::new(workflows))))
}

/// Adds every workflow in an export file as a new draft; nothing goes live
/// until it is checked (entity ids may differ here) and activated.
async fn import_workflows(
    State(s): AppStateRef,
    body: Result<Json<ExportFile>, axum::extract::rejection::JsonRejection>,
) -> ApiResult {
    let Json(file) = body.map_err(|e| {
        warn!(error = %e.body_text(), "import rejected");
        ApiError::bad(
            StatusCode::UNPROCESSABLE_ENTITY,
            "This isn't an ergo export file.",
        )
    })?;
    if file.format != EXPORT_FORMAT {
        return Err(ApiError::bad(
            StatusCode::UNPROCESSABLE_ENTITY,
            "This isn't an ergo export file.",
        ));
    }
    if file.version > EXPORT_VERSION {
        return Err(ApiError::bad(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "This file is from a newer ergo ({}); update this one first.",
                file.ergo_version
            ),
        ));
    }
    let workflows: Vec<(String, Graph)> = file
        .workflows
        .into_iter()
        .map(|wf| (wf.name.trim().to_string(), wf.draft))
        .collect();
    if workflows.iter().any(|(name, _)| name.is_empty()) {
        return Err(ApiError::bad(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Every workflow in the file needs a name.",
        ));
    }
    let created = s.db.import_workflows(&workflows)?;
    let created: Vec<Value> = created
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Ok(Json(json!({ "created": created })))
}

#[derive(Deserialize)]
struct RunsQuery {
    workflow: Option<String>,
    limit: Option<u32>,
}

async fn list_runs(State(s): AppStateRef, Query(q): Query<RunsQuery>) -> ApiResult {
    let limit = q.limit.unwrap_or(50).min(500);
    Ok(Json(json!(s.db.list_runs(q.workflow.as_deref(), limit)?)))
}

async fn get_run(State(s): AppStateRef, Path(id): Path<String>) -> ApiResult {
    let (run, nodes) = s.db.get_run(&id)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "run": run, "nodes": nodes })))
}

async fn nodes(State(s): AppStateRef) -> ApiResult {
    Ok(Json(json!(s.engine.registry().schemas())))
}

async fn entities(State(s): AppStateRef) -> ApiResult {
    Ok(Json(json!(s.ha.entities())))
}

#[derive(Deserialize)]
struct CronBody {
    cron: String,
}

async fn cron_preview(State(s): AppStateRef, Json(body): Json<CronBody>) -> ApiResult {
    match s.triggers.preview(&body.cron, 5) {
        Ok(next) => Ok(Json(
            json!({ "next": next, "time_zone": s.triggers.time_zone().name() }),
        )),
        Err(e) => Err(ApiError::bad(StatusCode::UNPROCESSABLE_ENTITY, e)),
    }
}

// Read-only database viewer.

async fn db_tables(State(s): AppStateRef) -> ApiResult {
    Ok(Json(json!({
        "path": s.db.path().display().to_string(),
        "tables": s.db.browse_tables()?,
        "retention": { "days": s.retention.0, "max_runs": s.retention.1 },
    })))
}

#[derive(Deserialize)]
struct RowsQuery {
    offset: Option<i64>,
    limit: Option<i64>,
}

async fn db_rows(
    State(s): AppStateRef,
    Path(name): Path<String>,
    Query(q): Query<RowsQuery>,
) -> ApiResult {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let page =
        s.db.browse_rows(&name, offset, limit)?
            .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!(page)))
}

// MQTT monitor: what ergo published, and what arrives on a watched filter.

#[derive(Deserialize)]
struct MessagesQuery {
    after: Option<u64>,
}

async fn mqtt_messages(State(s): AppStateRef, Query(q): Query<MessagesQuery>) -> ApiResult {
    Ok(Json(json!(s.mqtt.messages(q.after.unwrap_or(0)))))
}

async fn mqtt_clear(State(s): AppStateRef) -> ApiResult {
    s.mqtt.clear();
    Ok(Json(json!({ "cleared": true })))
}

#[derive(Deserialize)]
struct WatchBody {
    /// A topic filter such as `home/#`; null stops watching.
    filter: Option<String>,
}

async fn mqtt_watch(State(s): AppStateRef, Json(body): Json<WatchBody>) -> ApiResult {
    let filter = body
        .filter
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty());
    s.mqtt
        .watch(filter.clone())
        .await
        .map_err(|e| ApiError::bad(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    Ok(Json(json!({ "filter": filter })))
}
