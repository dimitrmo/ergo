//! Home Assistant WebSocket client.
//!
//! One connection subscribes to `state_changed` (real-time triggers), keeps a
//! cache of every entity's state (for the editor's pickers) and answers
//! commands. It reconnects with backoff and resyncs the cache after each
//! reconnect; events that happen while disconnected are not replayed.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use chrono_tz::Tz;
use ergo_nodes::{ActionCall, HaCaller};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct StateChanged {
    pub entity_id: String,
    pub old_state: Option<Value>,
    pub new_state: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HaStatus {
    pub connected: bool,
    pub error: Option<String>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub time_zone: Option<String>,
    pub version: Option<String>,
}

/// An entity as the editor's picker shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    pub entity_id: String,
    pub name: String,
    pub state: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Choices of an `input_select` / `select`, offered as "changes to" chips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

type Command = (Value, oneshot::Sender<Result<Value, String>>);

/// How long a call's context is remembered: state changes it causes arrive
/// well within this.
const CALL_MEMORY: Duration = Duration::from_secs(60);

pub struct Ha {
    ws_url: String,
    token: Option<String>,
    status: RwLock<HaStatus>,
    states: RwLock<BTreeMap<String, Value>>,
    events: broadcast::Sender<StateChanged>,
    commands: mpsc::Sender<Command>,
    /// Contexts of recent action calls and the workflow that made each, so a
    /// workflow isn't triggered again by the state changes it caused.
    calls: Mutex<VecDeque<(Instant, String, String)>>,
    /// Action calls waiting for HA's answer. HA sends the state changes a
    /// call causes before the answer that names its context.
    in_flight: AtomicUsize,
    /// The action catalog (`get_services`), fetched once per connection.
    services: RwLock<Option<Value>>,
    /// The time zone to use until HA says its own (ERGO_TZ, else UTC).
    fallback_tz: Tz,
}

/// `http://supervisor/core` becomes `ws://supervisor/core/websocket`;
/// `http://localhost:8123` becomes `ws://localhost:8123/api/websocket`.
pub fn websocket_url(ha_url: &str) -> Result<String> {
    let mut url = url::Url::parse(ha_url).with_context(|| format!("invalid HA URL {ha_url}"))?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        other => bail!("unsupported HA URL scheme {other}"),
    };
    url.set_scheme(scheme)
        .map_err(|_| anyhow!("invalid HA URL {ha_url}"))?;
    let path = url.path().trim_end_matches('/').to_string();
    let path = if path.ends_with("/core") {
        format!("{path}/websocket")
    } else {
        format!("{path}/api/websocket")
    };
    url.set_path(&path);
    Ok(url.to_string())
}

impl Ha {
    pub fn new(
        ha_url: &str,
        token: Option<String>,
        fallback_tz: Tz,
    ) -> Result<(Arc<Self>, mpsc::Receiver<Command>)> {
        let (events, _) = broadcast::channel(1024);
        let (commands, rx) = mpsc::channel(64);
        Ok((
            Arc::new(Self {
                ws_url: websocket_url(ha_url)?,
                token,
                status: RwLock::new(HaStatus::default()),
                states: RwLock::new(BTreeMap::new()),
                events,
                commands,
                calls: Mutex::new(VecDeque::new()),
                in_flight: AtomicUsize::new(0),
                services: RwLock::new(None),
                fallback_tz,
            }),
            rx,
        ))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StateChanged> {
        self.events.subscribe()
    }

    pub fn status(&self) -> HaStatus {
        self.status.read().unwrap().clone()
    }

    pub fn time_zone(&self) -> Option<String> {
        self.status.read().unwrap().time_zone.clone()
    }

    /// HA's configured zone once connected, else the fallback.
    pub fn tz(&self) -> Tz {
        self.time_zone()
            .and_then(|z| z.parse().ok())
            .unwrap_or(self.fallback_tz)
    }

    pub fn state(&self, entity_id: &str) -> Option<Value> {
        self.states.read().unwrap().get(entity_id).cloned()
    }

    pub fn entities(&self) -> Vec<Entity> {
        self.states
            .read()
            .unwrap()
            .iter()
            .map(|(id, s)| Entity {
                entity_id: id.clone(),
                name: s["attributes"]["friendly_name"]
                    .as_str()
                    .unwrap_or(id)
                    .to_string(),
                state: s["state"].as_str().unwrap_or_default().to_string(),
                domain: id.split('.').next().unwrap_or_default().to_string(),
                unit: s["attributes"]["unit_of_measurement"]
                    .as_str()
                    .map(str::to_string),
                options: s["attributes"]["options"].as_array().map(|o| {
                    o.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                }),
            })
            .collect()
    }

    /// The workflow whose action call caused a state change with this
    /// context id, if it was one of ergo's calls.
    pub fn caused_by(&self, context_id: &str) -> Option<String> {
        let mut calls = self.calls.lock().unwrap();
        while calls
            .front()
            .is_some_and(|(at, _, _)| at.elapsed() > CALL_MEMORY)
        {
            calls.pop_front();
        }
        calls
            .iter()
            .find(|(_, id, _)| id == context_id)
            .map(|(_, _, wf)| wf.clone())
    }

    /// Waits (up to `max`) until ergo's action calls have their answers, so
    /// [`Ha::caused_by`] knows every call's context.
    pub async fn calls_settled(&self, max: Duration) {
        let deadline = Instant::now() + max;
        while self.in_flight.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Every action HA offers, by domain, with its fields (for the editor).
    pub async fn services(&self) -> Result<Value, String> {
        if let Some(s) = self.services.read().unwrap().clone() {
            return Ok(s);
        }
        let s = self.call(json!({ "type": "get_services" })).await?;
        *self.services.write().unwrap() = Some(s.clone());
        Ok(s)
    }

    /// Sends a WebSocket command (without `id`) and waits for its result.
    pub async fn call(&self, msg: Value) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send((msg, tx))
            .await
            .map_err(|_| "HA client stopped".to_string())?;
        tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| "HA did not answer within 30 s".to_string())?
            .map_err(|_| "HA connection dropped".to_string())?
    }

    fn set_status(&self, f: impl FnOnce(&mut HaStatus)) {
        f(&mut self.status.write().unwrap());
    }

    /// Connects forever, backing off between attempts (1 s doubling to 30 s).
    pub async fn run(self: Arc<Self>, mut commands: mpsc::Receiver<Command>) {
        let mut backoff = Duration::from_secs(1);
        loop {
            match self.session(&mut commands).await {
                Ok(()) => {
                    info!("HA connection closed");
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    warn!(error = %e, retry_in = backoff.as_secs(), "HA connection failed");
                    self.set_status(|s| s.error = Some(e.to_string()));
                }
            }
            self.set_status(|s| s.connected = false);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }

    async fn session(&self, commands: &mut mpsc::Receiver<Command>) -> Result<()> {
        let Some(token) = &self.token else {
            bail!("no HA token: set ERGO_HA_TOKEN");
        };
        let (ws, _) = connect_async(&self.ws_url)
            .await
            .with_context(|| format!("connecting to {}", self.ws_url))?;
        let (mut tx, mut rx) = ws.split();

        // auth_required -> auth -> auth_ok
        let hello = next_json(&mut rx).await?;
        if hello["type"] != "auth_required" {
            bail!("unexpected greeting: {hello}");
        }
        tx.send(Message::Text(
            json!({ "type": "auth", "access_token": token })
                .to_string()
                .into(),
        ))
        .await?;
        let auth = next_json(&mut rx).await?;
        if auth["type"] != "auth_ok" {
            bail!(
                "authentication failed: {}",
                auth["message"].as_str().unwrap_or("rejected")
            );
        }
        let version = auth["ha_version"].as_str().map(str::to_string);

        const GET_CONFIG: u64 = 1;
        const GET_STATES: u64 = 2;
        const SUBSCRIBE: u64 = 3;
        for msg in [
            json!({ "id": GET_CONFIG, "type": "get_config" }),
            json!({ "id": GET_STATES, "type": "get_states" }),
            json!({ "id": SUBSCRIBE, "type": "subscribe_events", "event_type": "state_changed" }),
        ] {
            tx.send(Message::Text(msg.to_string().into())).await?;
        }
        let mut next_id = 10;
        let mut pending: HashMap<u64, oneshot::Sender<Result<Value, String>>> = HashMap::new();
        let mut ping = tokio::time::interval(Duration::from_secs(30));
        ping.tick().await;

        info!(url = %self.ws_url, ?version, "connected to Home Assistant");
        // Integrations may have changed while disconnected.
        *self.services.write().unwrap() = None;
        self.set_status(|s| {
            s.connected = true;
            s.error = None;
            s.version = version;
        });

        loop {
            tokio::select! {
                frame = rx.next() => {
                    let Some(frame) = frame else { return Ok(()) };
                    let text = match frame? {
                        Message::Text(t) => t,
                        Message::Close(_) => return Ok(()),
                        _ => continue,
                    };
                    let msg: Value = serde_json::from_str(&text)?;
                    let id = msg["id"].as_u64().unwrap_or_default();
                    match (msg["type"].as_str(), id) {
                        (Some("event"), SUBSCRIBE) => self.on_event(&msg["event"]),
                        (Some("result"), GET_CONFIG) => {
                            let tz = msg["result"]["time_zone"].as_str().map(str::to_string);
                            debug!(?tz, "HA config");
                            self.set_status(|s| s.time_zone = tz);
                        }
                        (Some("result"), GET_STATES) => self.load_states(&msg["result"]),
                        (Some("result"), SUBSCRIBE) => {
                            if msg["success"] != true {
                                bail!("subscribe failed: {}", msg["error"]);
                            }
                        }
                        (Some("result"), id) => {
                            if let Some(reply) = pending.remove(&id) {
                                let result = if msg["success"] == true {
                                    Ok(msg["result"].clone())
                                } else {
                                    Err(msg["error"]["message"].as_str().unwrap_or("HA error").to_string())
                                };
                                let _ = reply.send(result);
                            }
                        }
                        _ => {}
                    }
                }
                cmd = commands.recv() => {
                    let Some((mut msg, reply)) = cmd else { return Ok(()) };
                    next_id += 1;
                    msg["id"] = json!(next_id);
                    pending.insert(next_id, reply);
                    tx.send(Message::Text(msg.to_string().into())).await?;
                }
                _ = ping.tick() => {
                    next_id += 1;
                    tx.send(Message::Text(json!({ "id": next_id, "type": "ping" }).to_string().into())).await?;
                }
            }
        }
    }

    fn load_states(&self, result: &Value) {
        let mut states = self.states.write().unwrap();
        states.clear();
        for s in result.as_array().into_iter().flatten() {
            if let Some(id) = s["entity_id"].as_str() {
                states.insert(id.to_string(), s.clone());
            }
        }
        info!(entities = states.len(), "HA states loaded");
    }

    fn on_event(&self, event: &Value) {
        let data = &event["data"];
        let Some(entity_id) = data["entity_id"].as_str() else {
            return;
        };
        let new_state = data.get("new_state").filter(|v| !v.is_null()).cloned();
        {
            let mut states = self.states.write().unwrap();
            match &new_state {
                Some(s) => states.insert(entity_id.to_string(), s.clone()),
                None => states.remove(entity_id),
            };
        }
        self.set_status(|s| s.last_event_at = Some(Utc::now()));
        // No receivers is fine: nothing is listening yet.
        let _ = self.events.send(StateChanged {
            entity_id: entity_id.to_string(),
            old_state: data.get("old_state").filter(|v| !v.is_null()).cloned(),
            new_state,
        });
    }
}

#[async_trait]
impl HaCaller for Ha {
    async fn call_action(&self, call: ActionCall) -> Result<Value, String> {
        if !self.status().connected {
            return Err("Home Assistant isn't connected".into());
        }
        let mut msg = json!({
            "type": "call_service",
            "domain": call.domain,
            "service": call.service,
            "service_data": call.data,
            "return_response": call.return_response,
        });
        if !call.entity_ids.is_empty() {
            msg["target"] = json!({ "entity_id": call.entity_ids });
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        let result = self.call(msg).await;
        // Recorded before it stops counting as in flight.
        if let Some(id) = result
            .as_ref()
            .ok()
            .and_then(|r| r["context"]["id"].as_str())
        {
            let mut calls = self.calls.lock().unwrap();
            calls.push_back((Instant::now(), id.to_string(), call.workflow_id));
            // A burst of calls can't grow this without bound.
            while calls.len() > 1000 {
                calls.pop_front();
            }
        }
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        result
    }

    fn state(&self, entity_id: &str) -> Option<Value> {
        Ha::state(self, entity_id)
    }

    fn local_now(&self) -> DateTime<FixedOffset> {
        Utc::now().with_timezone(&self.tz()).fixed_offset()
    }
}

async fn next_json<S>(rx: &mut S) -> Result<Value>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match tokio::time::timeout(Duration::from_secs(15), rx.next()).await {
            Err(_) => bail!("timed out waiting for HA"),
            Ok(None) => bail!("connection closed"),
            Ok(Some(msg)) => {
                if let Message::Text(t) = msg? {
                    return Ok(serde_json::from_str(&t)?);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::websocket_url;

    #[test]
    fn websocket_urls() {
        assert_eq!(
            websocket_url("http://supervisor/core").unwrap(),
            "ws://supervisor/core/websocket"
        );
        assert_eq!(
            websocket_url("http://localhost:8123").unwrap(),
            "ws://localhost:8123/api/websocket"
        );
        assert_eq!(
            websocket_url("https://ha.example.com/").unwrap(),
            "wss://ha.example.com/api/websocket"
        );
    }
}
