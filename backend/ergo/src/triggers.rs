//! Connects active workflows to their triggers: HA state changes and cron.
//!
//! `reload` rebuilds everything from the database; it runs at startup and
//! whenever a workflow is activated, enabled, disabled or deleted.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use ergo_core::{Engine, Graph, RunRequest};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::db::Db;
use crate::ha::{Ha, StateChanged};

struct StateTrigger {
    workflow_id: String,
    version: i64,
    graph: Arc<Graph>,
    node_id: String,
    label: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

pub struct Triggers {
    db: Arc<Db>,
    engine: Arc<Engine>,
    ha: Arc<Ha>,
    fallback_tz: Tz,
    by_entity: RwLock<HashMap<String, Vec<Arc<StateTrigger>>>>,
    cron_tasks: Mutex<Vec<JoinHandle<()>>>,
}

fn filter(config: &Value, key: &str) -> Option<String> {
    config[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Triggers {
    pub fn new(db: Arc<Db>, engine: Arc<Engine>, ha: Arc<Ha>, fallback_tz: Tz) -> Arc<Self> {
        Arc::new(Self {
            db,
            engine,
            ha,
            fallback_tz,
            by_entity: RwLock::new(HashMap::new()),
            cron_tasks: Mutex::new(Vec::new()),
        })
    }

    /// HA's configured zone once connected, else ERGO_TZ, else UTC.
    pub fn time_zone(&self) -> Tz {
        self.ha
            .time_zone()
            .and_then(|z| z.parse().ok())
            .unwrap_or(self.fallback_tz)
    }

    pub fn reload(self: &Arc<Self>) -> Result<()> {
        let active = self.db.active_workflows()?;
        let mut by_entity: HashMap<String, Vec<Arc<StateTrigger>>> = HashMap::new();
        let mut tasks = self.cron_tasks.lock().unwrap();
        for task in tasks.drain(..) {
            task.abort();
        }
        let mut crons = 0;

        for wf in active {
            let graph = Arc::new(wf.graph);
            for node in &graph.nodes {
                match node.kind.as_str() {
                    "trigger.state" => {
                        let Some(entity_id) = filter(&node.config, "entity_id") else {
                            continue;
                        };
                        by_entity
                            .entry(entity_id)
                            .or_default()
                            .push(Arc::new(StateTrigger {
                                workflow_id: wf.id.clone(),
                                version: wf.version,
                                graph: graph.clone(),
                                node_id: node.id.clone(),
                                label: node.label.clone(),
                                from: filter(&node.config, "from"),
                                to: filter(&node.config, "to"),
                            }));
                    }
                    "trigger.cron" => {
                        let Some(expr) = filter(&node.config, "cron") else {
                            continue;
                        };
                        let Ok(cron) = Cron::from_str(&expr) else {
                            warn!(workflow = %wf.id, node = %node.id, "invalid cron, skipped");
                            continue;
                        };
                        crons += 1;
                        let this = self.clone();
                        let req = (
                            wf.id.clone(),
                            wf.version,
                            graph.clone(),
                            node.id.clone(),
                            node.label.clone(),
                        );
                        tasks.push(tokio::spawn(async move { this.run_cron(cron, req).await }));
                    }
                    _ => {}
                }
            }
        }
        let states: usize = by_entity.values().map(Vec::len).sum();
        *self.by_entity.write().unwrap() = by_entity;
        info!(
            state_triggers = states,
            cron_triggers = crons,
            "triggers loaded"
        );
        Ok(())
    }

    /// Matches HA state changes against state triggers, forever.
    pub async fn dispatch_state_changes(self: Arc<Self>) {
        let mut rx = self.ha.subscribe();
        loop {
            match rx.recv().await {
                Ok(ev) => self.on_state_changed(&ev),
                Err(RecvError::Lagged(n)) => {
                    warn!(dropped = n, "state events dropped (too many at once)")
                }
                Err(RecvError::Closed) => return,
            }
        }
    }

    fn on_state_changed(&self, ev: &StateChanged) {
        let triggers = {
            let index = self.by_entity.read().unwrap();
            match index.get(&ev.entity_id) {
                Some(t) => t.clone(),
                None => return,
            }
        };
        let old = ev.old_state.as_ref().and_then(|s| s["state"].as_str());
        let new = ev.new_state.as_ref().and_then(|s| s["state"].as_str());
        // Only real state changes fire; attribute-only updates don't.
        if old == new {
            return;
        }
        let time = Utc::now()
            .with_timezone(&self.time_zone())
            .format("%Y-%m-%d %H:%M")
            .to_string();
        for t in triggers {
            if t.from.as_deref().is_some_and(|f| Some(f) != old)
                || t.to.as_deref().is_some_and(|w| Some(w) != new)
            {
                continue;
            }
            debug!(workflow = %t.workflow_id, entity = %ev.entity_id, ?old, ?new, "state trigger fired");
            self.engine.start(RunRequest {
                workflow_id: t.workflow_id.clone(),
                version: t.version,
                graph: t.graph.clone(),
                trigger_node: t.node_id.clone(),
                trigger: json!({
                    "kind": "state",
                    "time": time,
                    "node": t.node_id,
                    "id": t.label,
                    "entity_id": ev.entity_id,
                    "from": old,
                    "to": new,
                    "from_state": ev.old_state,
                    "to_state": ev.new_state,
                }),
            });
        }
    }

    /// Fires one cron trigger on schedule. Sleeps in steps of at most a
    /// minute and recomputes, so clock jumps (NTP after a Pi boots without
    /// a hardware clock) and time zone changes are picked up.
    async fn run_cron(
        self: Arc<Self>,
        cron: Cron,
        req: (String, i64, Arc<Graph>, String, Option<String>),
    ) {
        let (workflow_id, version, graph, node_id, label) = req;
        let mut last_fired: Option<DateTime<Utc>> = None;
        loop {
            let tz = self.time_zone();
            let now = Utc::now().with_timezone(&tz);
            let Ok(next) = cron.find_next_occurrence(&now, false) else {
                warn!(workflow = %workflow_id, "cron has no next occurrence");
                return;
            };
            let wait = (next.with_timezone(&Utc) - Utc::now())
                .to_std()
                .unwrap_or_default();
            if wait > Duration::from_secs(60) {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
            tokio::time::sleep(wait).await;
            let due = next.with_timezone(&Utc);
            if last_fired == Some(due) {
                continue;
            }
            last_fired = Some(due);
            self.engine.start(RunRequest {
                workflow_id: workflow_id.clone(),
                version,
                graph: graph.clone(),
                trigger_node: node_id.clone(),
                trigger: json!({
                    "kind": "cron",
                    "time": next.format("%Y-%m-%d %H:%M").to_string(),
                    "node": node_id,
                    "id": label,
                    "scheduled": next.format("%Y-%m-%d %H:%M").to_string(),
                    "scheduled_iso": next.to_rfc3339(),
                }),
            });
            // Step past this minute so the same occurrence isn't found again.
            tokio::time::sleep(Duration::from_millis(1100)).await;
        }
    }

    /// The next `n` fire times of a cron expression, for the editor.
    pub fn preview(&self, expr: &str, n: usize) -> Result<Vec<String>, String> {
        let cron = Cron::from_str(expr).map_err(|e| e.to_string())?;
        let tz = self.time_zone();
        Ok(cron
            .iter_after(Utc::now().with_timezone(&tz))
            .take(n)
            .map(|t| t.to_rfc3339())
            .collect())
    }
}
