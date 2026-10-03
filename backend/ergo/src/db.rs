//! ergo's own SQLite database: workflows, versions and run history.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use ergo_core::{Graph, NodeRecord, RunSink, RunStart, RunStatus};
use ergo_nodes::{PushStore, PushSubscription};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use tracing::{error, info};

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE workflows (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        draft TEXT NOT NULL,
        active_version INTEGER,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE workflow_versions (
        workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
        version INTEGER NOT NULL,
        graph TEXT NOT NULL,
        activated_at TEXT NOT NULL,
        PRIMARY KEY (workflow_id, version)
    );
    CREATE TABLE runs (
        id TEXT PRIMARY KEY,
        workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
        version INTEGER NOT NULL,
        trigger_node TEXT NOT NULL,
        trigger TEXT NOT NULL,
        status TEXT NOT NULL,
        error TEXT,
        started_at TEXT NOT NULL,
        finished_at TEXT
    );
    CREATE INDEX runs_by_workflow ON runs (workflow_id, started_at DESC);
    CREATE TABLE run_nodes (
        run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
        seq INTEGER NOT NULL,
        node_id TEXT NOT NULL,
        node_type TEXT NOT NULL,
        port TEXT,
        input TEXT NOT NULL,
        output TEXT NOT NULL,
        error TEXT,
        started_at TEXT NOT NULL,
        duration_ms INTEGER NOT NULL,
        PRIMARY KEY (run_id, seq)
    );
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    // v2: step records hold the input, the rendered config, a typed error,
    // attempts and logs. v1's `input` column held the rendered config.
    "ALTER TABLE run_nodes ADD COLUMN config TEXT NOT NULL DEFAULT 'null';
    ALTER TABLE run_nodes ADD COLUMN attempts TEXT NOT NULL DEFAULT '[]';
    ALTER TABLE run_nodes ADD COLUMN logs TEXT NOT NULL DEFAULT '[]';
    UPDATE run_nodes SET config = input, input = 'null';
    UPDATE run_nodes SET error = json_object('kind', 'other', 'message', error) WHERE error IS NOT NULL;",
    // v3: browsers that turned on web push notifications.
    "CREATE TABLE push_subscriptions (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        endpoint TEXT NOT NULL UNIQUE,
        p256dh TEXT NOT NULL,
        auth TEXT NOT NULL,
        created_at TEXT NOT NULL
    );",
];

/// Activated versions kept per workflow, for rollback and run replay.
const VERSIONS_KEPT: i64 = 20;

pub struct Db {
    conn: Mutex<Connection>,
    path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct TableInfo {
    pub name: String,
    pub rows: i64,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub decl_type: String,
    pub pk: bool,
}

#[derive(Debug, Serialize)]
pub struct TablePage {
    pub table: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Serialize)]
pub struct LastRun {
    pub id: String,
    pub status: String,
    pub started_at: String,
}

#[derive(Debug, Serialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub draft: Graph,
    pub active_version: Option<i64>,
    /// The draft differs from the active version (or nothing is active yet).
    pub dirty: bool,
    /// The live version's manual trigger, if it has one (for "Run now").
    pub manual_trigger: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_run: Option<LastRun>,
}

#[derive(Debug, Serialize)]
pub struct Run {
    pub id: String,
    pub workflow_id: String,
    pub version: i64,
    pub trigger_node: String,
    pub trigger: Value,
    pub status: String,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunNode {
    pub seq: u32,
    pub node_id: String,
    pub node_type: String,
    pub port: Option<String>,
    pub input: Value,
    /// The step's config after templates were rendered.
    pub config: Value,
    pub output: Value,
    /// `{ kind, message, details? }`
    pub error: Option<Value>,
    pub attempts: Value,
    pub logs: Value,
    pub started_at: String,
    pub duration_ms: u64,
}

/// An enabled workflow's active version, as the trigger manager needs it.
pub struct Active {
    pub id: String,
    pub version: i64,
    pub graph: Graph,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serializable")
}

fn parse<T: serde::de::DeserializeOwned + Default>(text: &str) -> T {
    serde_json::from_str(text).unwrap_or_default()
}

impl Db {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join("ergo.db");
        let conn =
            Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;",
        )?;
        let db = Self {
            conn: Mutex::new(conn),
            path,
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!(
                "BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))
            .with_context(|| format!("database migration {}", i + 1))?;
            info!(version = i + 1, "database migrated");
        }
        Ok(())
    }

    pub fn list_workflows(&self) -> Result<Vec<Workflow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(WORKFLOW_SELECT_ALL)?;
        let rows = stmt.query_map([], workflow_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get_workflow(&self, id: &str) -> Result<Option<Workflow>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(WORKFLOW_SELECT_ONE, [id], workflow_from_row)
            .optional()?)
    }

    pub fn create_workflow(&self, id: &str, name: &str, draft: &Graph) -> Result<()> {
        let t = now();
        self.conn.lock().unwrap().execute(
            "INSERT INTO workflows (id, name, enabled, draft, created_at, updated_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?4)",
            params![id, name, json(draft), t],
        )?;
        Ok(())
    }

    /// Creates each (name, draft) as a new workflow, in one transaction, and
    /// returns their (id, name). A name already taken gets " (imported)".
    pub fn import_workflows(&self, workflows: &[(String, Graph)]) -> Result<Vec<(String, String)>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let t = now();
        let mut created = Vec::new();
        for (name, draft) in workflows {
            let taken: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM workflows WHERE name = ?1)",
                [name],
                |r| r.get(0),
            )?;
            let name = if taken {
                format!("{name} (imported)")
            } else {
                name.clone()
            };
            let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            tx.execute(
                "INSERT INTO workflows (id, name, enabled, draft, created_at, updated_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?4)",
                params![id, name, json(draft), t],
            )?;
            created.push((id, name));
        }
        tx.commit()?;
        Ok(created)
    }

    /// Copies a workflow's draft into a new workflow named "<name> (copy)",
    /// or "(copy 2)" and so on when that's taken. The copy starts as a draft,
    /// with no live version and no runs. Returns its id, or None if `id`
    /// doesn't exist.
    pub fn duplicate_workflow(&self, id: &str) -> Result<Option<String>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let Some((name, draft)) = tx
            .query_row(
                "SELECT name, draft FROM workflows WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let base = copy_base(&name);
        let mut n = 1;
        let name = loop {
            let candidate = if n == 1 {
                format!("{base} (copy)")
            } else {
                format!("{base} (copy {n})")
            };
            let taken: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM workflows WHERE name = ?1)",
                [&candidate],
                |r| r.get(0),
            )?;
            if !taken {
                break candidate;
            }
            n += 1;
        };
        let new_id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        tx.execute(
            "INSERT INTO workflows (id, name, enabled, draft, created_at, updated_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?4)",
            params![new_id, name, draft, now()],
        )?;
        tx.commit()?;
        Ok(Some(new_id))
    }

    /// Saves the draft and/or name. Returns false if the workflow doesn't exist.
    pub fn save_draft(&self, id: &str, name: Option<&str>, draft: Option<&Graph>) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE workflows SET name = COALESCE(?2, name), draft = COALESCE(?3, draft), updated_at = ?4
             WHERE id = ?1",
            params![id, name, draft.map(json), now()],
        )?;
        Ok(n > 0)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool> {
        let n = self.conn.lock().unwrap().execute(
            "UPDATE workflows SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, enabled, now()],
        )?;
        Ok(n > 0)
    }

    pub fn delete_workflow(&self, id: &str) -> Result<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM workflows WHERE id = ?1", [id])?;
        Ok(n > 0)
    }

    /// Makes the current draft the active version and returns its number.
    pub fn activate(&self, id: &str, graph: &Graph) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let version: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM workflow_versions WHERE workflow_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO workflow_versions (workflow_id, version, graph, activated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, version, json(graph), now()],
        )?;
        tx.execute(
            "UPDATE workflows SET active_version = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, version, now()],
        )?;
        tx.execute(
            "DELETE FROM workflow_versions WHERE workflow_id = ?1 AND version <= ?2",
            params![id, version - VERSIONS_KEPT],
        )?;
        tx.commit()?;
        Ok(version)
    }

    pub fn active_workflows(&self) -> Result<Vec<Active>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT w.id, v.version, v.graph FROM workflows w
             JOIN workflow_versions v ON v.workflow_id = w.id AND v.version = w.active_version
             WHERE w.enabled = 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Active {
                id: r.get(0)?,
                version: r.get(1)?,
                graph: parse(&r.get::<_, String>(2)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn active_graph(&self, id: &str) -> Result<Option<(i64, Graph)>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT v.version, v.graph FROM workflows w
                 JOIN workflow_versions v ON v.workflow_id = w.id AND v.version = w.active_version
                 WHERE w.id = ?1",
                [id],
                |r| Ok((r.get(0)?, parse(&r.get::<_, String>(1)?))),
            )
            .optional()?)
    }

    pub fn list_runs(&self, workflow_id: Option<&str>, limit: u32) -> Result<Vec<Run>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, workflow_id, version, trigger_node, trigger, status, error, started_at, finished_at
             FROM runs WHERE (?1 IS NULL OR workflow_id = ?1)
             ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![workflow_id, limit], run_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get_run(&self, id: &str) -> Result<Option<(Run, Vec<RunNode>)>> {
        let conn = self.conn.lock().unwrap();
        let Some(run) = conn
            .query_row(
                "SELECT id, workflow_id, version, trigger_node, trigger, status, error, started_at, finished_at
                 FROM runs WHERE id = ?1",
                [id],
                run_from_row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let mut stmt = conn.prepare(
            "SELECT seq, node_id, node_type, port, input, config, output, error, attempts, logs, started_at, duration_ms
             FROM run_nodes WHERE run_id = ?1 ORDER BY seq",
        )?;
        let nodes = stmt
            .query_map([id], |r| {
                Ok(RunNode {
                    seq: r.get(0)?,
                    node_id: r.get(1)?,
                    node_type: r.get(2)?,
                    port: r.get(3)?,
                    input: parse(&r.get::<_, String>(4)?),
                    config: parse(&r.get::<_, String>(5)?),
                    output: parse(&r.get::<_, String>(6)?),
                    error: r.get::<_, Option<String>>(7)?.map(|e| parse(&e)),
                    attempts: parse(&r.get::<_, String>(8)?),
                    logs: parse(&r.get::<_, String>(9)?),
                    started_at: r.get(10)?,
                    duration_ms: r.get::<_, i64>(11)? as u64,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(Some((run, nodes)))
    }

    pub fn push_subscriptions(&self) -> Result<Vec<PushSubscription>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, endpoint, p256dh, auth FROM push_subscriptions ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PushSubscription {
                id: r.get(0)?,
                name: r.get(1)?,
                endpoint: r.get(2)?,
                p256dh: r.get(3)?,
                auth: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Adds a browser, or renames it and refreshes its keys when its
    /// endpoint is already known. Returns its id.
    pub fn save_push_subscription(&self, sub: &PushSubscription) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "INSERT INTO push_subscriptions (id, name, endpoint, p256dh, auth, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (endpoint) DO UPDATE SET name = ?2, p256dh = ?4, auth = ?5
             RETURNING id",
            params![
                sub.id,
                sub.name,
                sub.endpoint,
                sub.p256dh,
                sub.auth,
                Utc::now().to_rfc3339()
            ],
            |r| r.get(0),
        )?)
    }

    /// Returns whether it was there.
    pub fn delete_push_subscription(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM push_subscriptions WHERE id = ?1", [id])? > 0)
    }

    /// Deletes runs past the retention window or count. Returns how many.
    pub fn prune_runs(&self, days: u32, max: u32) -> Result<usize> {
        let cutoff = (Utc::now() - Duration::days(days as i64)).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let mut n = conn.execute("DELETE FROM runs WHERE started_at < ?1", [cutoff])?;
        n += conn.execute(
            "DELETE FROM runs WHERE id NOT IN (SELECT id FROM runs ORDER BY started_at DESC LIMIT ?1)",
            [max],
        )?;
        Ok(n)
    }

    /// Marks runs that were in flight when ergo stopped.
    pub fn mark_interrupted(&self) -> Result<usize> {
        Ok(self.conn.lock().unwrap().execute(
            "UPDATE runs SET status = 'interrupted', finished_at = ?1 WHERE status = 'running'",
            [now()],
        )?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn ping(&self) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }

    /// A read-only connection for the database viewer: it cannot write, even by mistake.
    fn read_only(&self) -> Result<Connection> {
        Ok(Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?)
    }

    /// User tables with their columns and row counts.
    pub fn browse_tables(&self) -> Result<Vec<TableInfo>> {
        let conn = self.read_only()?;
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        names
            .into_iter()
            .map(|name| {
                let quoted = quote_ident(&name);
                let rows =
                    conn.query_row(&format!("SELECT COUNT(*) FROM {quoted}"), [], |r| r.get(0))?;
                let columns = conn
                    .prepare(&format!("PRAGMA table_info({quoted})"))?
                    .query_map([], |r| {
                        Ok(ColumnInfo {
                            name: r.get(1)?,
                            decl_type: r.get(2)?,
                            pk: r.get::<_, i64>(5)? > 0,
                        })
                    })?
                    .collect::<Result<_, _>>()?;
                Ok(TableInfo {
                    name,
                    rows,
                    columns,
                })
            })
            .collect()
    }

    /// One page of a table, newest rows first. `table` must be a real table
    /// name (checked against sqlite_master), so nothing else can be queried.
    pub fn browse_rows(&self, table: &str, offset: i64, limit: i64) -> Result<Option<TablePage>> {
        let conn = self.read_only()?;
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1 AND name NOT LIKE 'sqlite_%'",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        let quoted = quote_ident(table);
        let total = conn.query_row(&format!("SELECT COUNT(*) FROM {quoted}"), [], |r| r.get(0))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM {quoted} ORDER BY rowid DESC LIMIT ?1 OFFSET ?2"
        ))?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let n = columns.len();
        let rows = stmt
            .query_map(params![limit, offset], |r| {
                (0..n).map(|i| Ok(to_json(r.get_ref(i)?))).collect()
            })?
            .collect::<Result<_, _>>()?;
        Ok(Some(TablePage {
            table: table.to_string(),
            columns,
            rows,
            total,
            offset,
            limit,
        }))
    }

    fn log_err(result: rusqlite::Result<usize>, what: &str) {
        if let Err(e) = result {
            error!(error = %e, "recording {what}");
        }
    }
}

// The last run shown in lists is the last real one (version > 0), not a test run.
macro_rules! workflow_select {
    ($where:literal) => {
        concat!(
            "SELECT w.id, w.name, w.enabled, w.draft, w.active_version, w.created_at, w.updated_at,
                (SELECT v.graph FROM workflow_versions v
                 WHERE v.workflow_id = w.id AND v.version = w.active_version),
                r.id, r.status, r.started_at
             FROM workflows w
             LEFT JOIN runs r ON r.id = (SELECT id FROM runs
                 WHERE workflow_id = w.id AND version > 0 ORDER BY started_at DESC LIMIT 1) ",
            $where
        )
    };
}

const WORKFLOW_SELECT_ALL: &str = workflow_select!("ORDER BY w.name COLLATE NOCASE");
const WORKFLOW_SELECT_ONE: &str = workflow_select!("WHERE w.id = ?1");

fn workflow_from_row(r: &rusqlite::Row) -> rusqlite::Result<Workflow> {
    let draft_text: String = r.get(3)?;
    let active_text: Option<String> = r.get(7)?;
    let draft: Graph = parse(&draft_text);
    let active: Option<Graph> = active_text.as_deref().map(parse);
    let dirty = active.as_ref() != Some(&draft);
    let manual_trigger = active
        .as_ref()
        .and_then(|g| g.nodes.iter().find(|n| n.kind == "trigger.manual"))
        .map(|n| n.id.clone());
    let last_run = match r.get::<_, Option<String>>(8)? {
        Some(id) => Some(LastRun {
            id,
            status: r.get(9)?,
            started_at: r.get(10)?,
        }),
        None => None,
    };
    Ok(Workflow {
        id: r.get(0)?,
        name: r.get(1)?,
        enabled: r.get(2)?,
        draft,
        active_version: r.get(4)?,
        dirty,
        manual_trigger,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        last_run,
    })
}

fn run_from_row(r: &rusqlite::Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        workflow_id: r.get(1)?,
        version: r.get(2)?,
        trigger_node: r.get(3)?,
        trigger: parse(&r.get::<_, String>(4)?),
        status: r.get(5)?,
        error: r.get(6)?,
        started_at: r.get(7)?,
        finished_at: r.get(8)?,
    })
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn to_json(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Value::from(f),
        ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::from(format!("<{} bytes>", b.len())),
    }
}

fn ts(t: &DateTime<Utc>) -> String {
    t.to_rfc3339()
}

impl PushStore for Db {
    fn subscriptions(&self) -> std::result::Result<Vec<PushSubscription>, String> {
        self.push_subscriptions().map_err(|e| e.to_string())
    }

    fn forget(&self, id: &str) {
        if let Err(e) = self.delete_push_subscription(id) {
            error!(error = %e, "forgetting a push subscription");
        }
    }
}

impl RunSink for Db {
    fn run_started(&self, run: &RunStart) {
        Self::log_err(
            self.conn.lock().unwrap().execute(
                "INSERT INTO runs (id, workflow_id, version, trigger_node, trigger, status, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6)",
                params![run.id, run.workflow_id, run.version, run.trigger_node, json(&run.trigger), ts(&run.started_at)],
            ),
            "run start",
        );
    }

    fn node_finished(&self, n: &NodeRecord) {
        Self::log_err(
            self.conn.lock().unwrap().execute(
                "INSERT INTO run_nodes (run_id, seq, node_id, node_type, port, input, config, output, error, attempts, logs, started_at, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    n.run_id, n.seq, n.node_id, n.node_type, n.port, json(&n.input), json(&n.config), json(&n.output),
                    n.error.as_ref().map(json), json(&n.attempts), json(&n.logs), ts(&n.started_at), n.duration_ms as i64
                ],
            ),
            "node result",
        );
    }

    fn run_finished(&self, run_id: &str, status: RunStatus, error: Option<&str>) {
        Self::log_err(
            self.conn.lock().unwrap().execute(
                "UPDATE runs SET status = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
                params![run_id, status.as_str(), error, now()],
            ),
            "run finish",
        );
    }

    fn run_skipped(&self, run: &RunStart, reason: &str) {
        Self::log_err(
            self.conn.lock().unwrap().execute(
                "INSERT INTO runs (id, workflow_id, version, trigger_node, trigger, status, error, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'skipped', ?6, ?7, ?7)",
                params![run.id, run.workflow_id, run.version, run.trigger_node, json(&run.trigger), reason, ts(&run.started_at)],
            ),
            "skipped run",
        );
    }
}

/// "Lights (copy 2)" -> "Lights", so copies of copies don't pile up suffixes.
fn copy_base(name: &str) -> &str {
    let Some(rest) = name.strip_suffix(')') else {
        return name;
    };
    let Some(i) = rest.rfind(" (copy") else {
        return name;
    };
    let tail = &rest[i + " (copy".len()..];
    if tail.is_empty()
        || tail
            .strip_prefix(' ')
            .is_some_and(|d| d.parse::<u32>().is_ok())
    {
        &name[..i]
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_core::{NodeRecord, RunSink, RunStart, RunStatus};

    fn temp_db() -> Db {
        let dir = std::env::temp_dir().join(format!("ergo-test-{}", uuid::Uuid::new_v4()));
        Db::open(&dir).unwrap()
    }

    fn add_run(db: &Db, wf: &str, started: DateTime<Utc>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        db.run_started(&RunStart {
            id: id.clone(),
            workflow_id: wf.into(),
            version: 1,
            trigger_node: "n1".into(),
            trigger: serde_json::json!({}),
            started_at: started,
        });
        db.node_finished(&NodeRecord {
            run_id: id.clone(),
            seq: 1,
            node_id: "n1".into(),
            node_type: "trigger.manual".into(),
            port: Some("out".into()),
            input: Value::Null,
            config: Value::Null,
            output: Value::Null,
            error: None,
            attempts: vec![],
            logs: vec![],
            started_at: started,
            duration_ms: 0,
        });
        db.run_finished(&id, RunStatus::Success, None);
        id
    }

    fn count(db: &Db, table: &str) -> i64 {
        db.conn
            .lock()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn retention_prunes_old_and_excess_runs_with_their_steps() {
        let db = temp_db();
        db.create_workflow("wf", "Test", &Graph::default()).unwrap();
        add_run(&db, "wf", Utc::now() - Duration::days(30));
        for i in 0..5 {
            add_run(&db, "wf", Utc::now() - Duration::minutes(i));
        }
        assert_eq!(count(&db, "runs"), 6);

        // 14 days and at most 3 runs: the old one and the two oldest recent ones go.
        assert_eq!(db.prune_runs(14, 3).unwrap(), 3);
        assert_eq!(count(&db, "runs"), 3);
        assert_eq!(
            count(&db, "run_nodes"),
            3,
            "step records are deleted with their run"
        );
    }

    #[test]
    fn migration_v2_converts_old_step_records() {
        let dir = std::env::temp_dir().join(format!("ergo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        {
            // A v1 database with one old-style step: `input` held the config, errors were text.
            let conn = Connection::open(dir.join("ergo.db")).unwrap();
            conn.execute_batch(&format!("{}; PRAGMA user_version = 1;", MIGRATIONS[0]))
                .unwrap();
            conn.execute_batch(
                "INSERT INTO workflows VALUES ('wf', 'Old', 1, '{}', NULL, 'x', 'x');
                 INSERT INTO runs VALUES ('r1', 'wf', 1, 'n1', '{}', 'failed', 'n2: boom', 'x', 'x');
                 INSERT INTO run_nodes VALUES ('r1', 1, 'n2', 'mqtt.publish', NULL, '{\"topic\":\"a/b\"}', 'null', 'boom', 'x', 3);",
            )
            .unwrap();
        }
        let db = Db::open(&dir).unwrap();
        let (_, nodes) = db.get_run("r1").unwrap().unwrap();
        assert_eq!(nodes[0].config, serde_json::json!({ "topic": "a/b" }));
        assert_eq!(nodes[0].input, Value::Null);
        assert_eq!(
            nodes[0].error,
            Some(serde_json::json!({ "kind": "other", "message": "boom" }))
        );
        assert_eq!(nodes[0].attempts, serde_json::json!([]));
    }

    #[test]
    fn import_creates_drafts_and_renames_clashes() {
        let db = temp_db();
        db.create_workflow("wf", "Lights", &Graph::default())
            .unwrap();
        let created = db
            .import_workflows(&[
                ("Lights".into(), Graph::default()),
                ("Heater".into(), Graph::default()),
            ])
            .unwrap();
        let names: Vec<_> = created.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, ["Lights (imported)", "Heater"]);
        let wf = db.get_workflow(&created[1].0).unwrap().unwrap();
        assert_eq!(
            wf.active_version, None,
            "imports are drafts until they go live"
        );
        assert_eq!(count(&db, "workflows"), 3);
    }

    #[test]
    fn duplicate_copies_the_draft_under_a_free_name() {
        let db = temp_db();
        let graph = Graph::default();
        db.create_workflow("wf", "Lights", &graph).unwrap();
        db.activate("wf", &graph).unwrap();

        let first = db.duplicate_workflow("wf").unwrap().unwrap();
        let second = db.duplicate_workflow("wf").unwrap().unwrap();
        let third = db.duplicate_workflow(&first).unwrap().unwrap();
        let name = |id: &str| db.get_workflow(id).unwrap().unwrap().name;
        assert_eq!(name(&first), "Lights (copy)");
        assert_eq!(name(&second), "Lights (copy 2)");
        assert_eq!(name(&third), "Lights (copy 3)");

        let copy = db.get_workflow(&first).unwrap().unwrap();
        assert_eq!(
            copy.active_version, None,
            "copies are drafts until they go live"
        );
        assert_eq!(copy.draft, graph);
        assert!(db.duplicate_workflow("nope").unwrap().is_none());
    }

    #[test]
    fn copy_base_strips_only_copy_suffixes() {
        assert_eq!(copy_base("Lights (copy)"), "Lights");
        assert_eq!(copy_base("Lights (copy 12)"), "Lights");
        assert_eq!(copy_base("Lights (copycat)"), "Lights (copycat)");
        assert_eq!(copy_base("Lights (copy x)"), "Lights (copy x)");
        assert_eq!(copy_base("Lights"), "Lights");
    }

    #[test]
    fn viewer_rejects_unknown_tables_and_reads_real_ones() {
        let db = temp_db();
        db.create_workflow("wf", "Test", &Graph::default()).unwrap();
        assert!(db.browse_rows("sqlite_master", 0, 10).unwrap().is_none());
        assert!(
            db.browse_rows("nope\"; DROP TABLE runs; --", 0, 10)
                .unwrap()
                .is_none()
        );
        let page = db.browse_rows("workflows", 0, 10).unwrap().unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0][1], Value::from("Test"));
    }
}
