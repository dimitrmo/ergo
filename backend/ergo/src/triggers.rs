//! Connects active workflows to their triggers: HA state changes, cron and
//! MQTT messages.
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
use crate::mqtt::{Incoming, Mqtt};

struct StateTrigger {
    workflow_id: String,
    version: i64,
    graph: Arc<Graph>,
    node_id: String,
    label: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

struct MqttTrigger {
    workflow_id: String,
    version: i64,
    graph: Arc<Graph>,
    node_id: String,
    label: Option<String>,
    filter: String,
    payload: Option<String>,
}

pub struct Triggers {
    db: Arc<Db>,
    engine: Arc<Engine>,
    ha: Arc<Ha>,
    /// None when MQTT is off; MQTT triggers then don't load.
    mqtt: Option<Arc<Mqtt>>,
    by_entity: RwLock<HashMap<String, Vec<Arc<StateTrigger>>>>,
    mqtt_triggers: RwLock<Vec<Arc<MqttTrigger>>>,
    cron_tasks: Mutex<Vec<JoinHandle<()>>>,
}

/// The event an MQTT trigger hands its run: the message as text, and as JSON
/// when it is JSON.
pub fn mqtt_event(topic: &str, payload: &[u8], qos: u8) -> Value {
    let text = String::from_utf8_lossy(payload).into_owned();
    let parsed = serde_json::from_slice::<Value>(payload).ok();
    json!({
        "kind": "mqtt",
        "topic": topic,
        "payload": text,
        "json": parsed,
        "qos": qos,
    })
}

fn filter(config: &Value, key: &str) -> Option<String> {
    config[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Like [`filter`], but keeps surrounding spaces: an MQTT message is matched
/// exactly.
fn filter_raw(config: &Value, key: &str) -> Option<String> {
    config[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Triggers {
    pub fn new(
        db: Arc<Db>,
        engine: Arc<Engine>,
        ha: Arc<Ha>,
        mqtt: Option<Arc<Mqtt>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            engine,
            ha,
            mqtt,
            by_entity: RwLock::new(HashMap::new()),
            mqtt_triggers: RwLock::new(Vec::new()),
            cron_tasks: Mutex::new(Vec::new()),
        })
    }

    /// HA's configured zone once connected, else ERGO_TZ, else UTC.
    pub fn time_zone(&self) -> Tz {
        self.ha.tz()
    }

    pub fn reload(self: &Arc<Self>) -> Result<()> {
        let active = self.db.active_workflows()?;
        let mut by_entity: HashMap<String, Vec<Arc<StateTrigger>>> = HashMap::new();
        let mut mqtt_triggers = Vec::new();
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
                    "trigger.mqtt" if self.mqtt.is_some() => {
                        let Some(filter) = filter(&node.config, "topic") else {
                            continue;
                        };
                        if let Err(e) = ergo_nodes::valid_topic_filter(&filter) {
                            warn!(workflow = %wf.id, node = %node.id, error = %e, "invalid MQTT topic, skipped");
                            continue;
                        }
                        mqtt_triggers.push(Arc::new(MqttTrigger {
                            workflow_id: wf.id.clone(),
                            version: wf.version,
                            graph: graph.clone(),
                            node_id: node.id.clone(),
                            label: node.label.clone(),
                            filter,
                            payload: filter_raw(&node.config, "payload"),
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
        if let Some(mqtt) = &self.mqtt {
            mqtt.set_trigger_filters(mqtt_triggers.iter().map(|t| t.filter.clone()));
        }
        let mqtts = mqtt_triggers.len();
        *self.mqtt_triggers.write().unwrap() = mqtt_triggers;
        info!(
            state_triggers = states,
            cron_triggers = crons,
            mqtt_triggers = mqtts,
            "triggers loaded"
        );
        Ok(())
    }

    /// Matches HA state changes against state triggers, forever.
    pub async fn dispatch_state_changes(self: Arc<Self>) {
        let mut rx = self.ha.subscribe();
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    // Lets the loop guard see the context of a call that is
                    // still waiting for its answer.
                    self.ha.calls_settled(Duration::from_secs(1)).await;
                    self.on_state_changed(&ev)
                }
                Err(RecvError::Lagged(n)) => {
                    warn!(dropped = n, "state events dropped (too many at once)")
                }
                Err(RecvError::Closed) => return,
            }
        }
    }

    /// Matches MQTT messages against MQTT triggers, forever.
    pub async fn dispatch_mqtt_messages(self: Arc<Self>) {
        let Some(mqtt) = &self.mqtt else { return };
        let mut rx = mqtt.subscribe();
        loop {
            match rx.recv().await {
                Ok(msg) => self.on_mqtt_message(&msg),
                Err(RecvError::Lagged(n)) => {
                    warn!(dropped = n, "MQTT messages dropped (too many at once)")
                }
                Err(RecvError::Closed) => return,
            }
        }
    }

    fn on_mqtt_message(&self, msg: &Incoming) {
        // Retained messages come when a trigger subscribes (a restart, a
        // workflow going live); they're old news, not something happening.
        if msg.retain {
            return;
        }
        let triggers: Vec<_> = self
            .mqtt_triggers
            .read()
            .unwrap()
            .iter()
            .filter(|t| ergo_nodes::topic_matches(&t.filter, &msg.topic))
            .cloned()
            .collect();
        if triggers.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&msg.payload);
        let time = Utc::now()
            .with_timezone(&self.time_zone())
            .format("%Y-%m-%d %H:%M")
            .to_string();
        for t in triggers {
            if t.payload.as_deref().is_some_and(|p| p != text) {
                continue;
            }
            debug!(workflow = %t.workflow_id, topic = %msg.topic, "MQTT trigger fired");
            let mut trigger = mqtt_event(&msg.topic, &msg.payload, msg.qos);
            trigger["time"] = json!(time);
            trigger["node"] = json!(t.node_id);
            trigger["id"] = json!(t.label);
            self.engine.start(RunRequest {
                workflow_id: t.workflow_id.clone(),
                version: t.version,
                graph: t.graph.clone(),
                trigger_node: t.node_id.clone(),
                trigger,
            });
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
        // A change one of ergo's action calls caused; its workflow must not
        // trigger itself with it (a light toggling itself forever).
        let caused_by = ev
            .new_state
            .as_ref()
            .and_then(|s| s["context"]["id"].as_str())
            .and_then(|id| self.ha.caused_by(id));
        for t in triggers {
            if t.from.as_deref().is_some_and(|f| Some(f) != old)
                || t.to.as_deref().is_some_and(|w| Some(w) != new)
            {
                continue;
            }
            if caused_by.as_deref() == Some(t.workflow_id.as_str()) {
                info!(workflow = %t.workflow_id, entity = %ev.entity_id, "ignored a change this workflow caused itself");
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
