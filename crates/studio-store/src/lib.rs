mod schema;

pub use schema::SCHEMA_VERSION;

use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use studio_events::{Envelope, EventType, Outcome, Scene, Usage, WorkerState};

const READER_POOL: usize = 4;

fn wire_index() -> &'static HashMap<&'static str, EventType> {
    static INDEX: OnceLock<HashMap<&'static str, EventType>> = OnceLock::new();
    INDEX.get_or_init(|| EventType::ALL.iter().map(|t| (t.wire_name(), *t)).collect())
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("the writer actor is gone")]
    WriterGone,
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    pub root: String,
    pub engine: String,
    pub git: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Purged {
    pub tasks: usize,
    pub events: usize,
    pub capsules: usize,
    pub ledger_rows: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoleRow {
    pub id: String,
    pub tier: u8,
    pub department: String,
    pub model: String,
    pub effort: String,
    pub escalates_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub id: String,
    pub run: String,
    pub role: String,
    pub parent_task: Option<String>,
    pub workflow_node: Option<String>,
    pub state: WorkerState,
    pub outcome: Option<Outcome>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub session_id: String,
    pub task: String,
    pub prefix_hash: String,
    pub forked_from: Option<String>,
    pub jsonl_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapsuleRow {
    pub id: String,
    pub task: String,
    pub kind: String,
    pub from_role: String,
    pub outcome: String,
    pub rendered_tokens: usize,
    pub truncated: bool,
    pub body_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRow {
    pub id: String,
    pub title: String,
    pub claim: String,
    pub rationale: String,
    pub origin_capsule: Option<String>,
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub task: String,
    pub role: String,
    pub prefix_hash: String,
    pub estimate: bool,
    pub usage: Usage,
    pub cost_usd: f64,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spend {
    pub tokens: u64,
    pub usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheHealth {
    pub role: String,
    pub prefix_hash: String,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl CacheHealth {
    pub fn hit_ratio(&self) -> Option<f64> {
        let total = self.cache_read + self.cache_creation;
        if total == 0 {
            None
        } else {
            Some(self.cache_read as f64 / total as f64)
        }
    }
}

enum Cmd {
    UpsertRole(RoleRow, Reply<()>),
    InsertProject(ProjectRow, String, Reply<()>),
    TouchProject(String, String, Reply<()>),
    ForgetProject(String, String, Reply<bool>),
    PurgeProject(String, Reply<Purged>),
    InsertCapsule(CapsuleRow, String, Reply<()>),
    InsertDecision(DecisionRow, String, Reply<()>),
    InsertTask(TaskRow, String, Reply<()>),
    UpdateTaskState(String, WorkerState, Option<Outcome>, String, Reply<()>),
    InsertSession(SessionRow, String, Reply<()>),
    AppendEvent {
        run: String,
        ts: String,
        actor: String,
        event_type: EventType,
        scene: Scene,
        data: serde_json::Value,
        reply: Reply<Envelope>,
    },
    RecordUsage(LedgerEntry, String, Reply<()>),
    Checkpoint(Reply<u64>),
    Shutdown,
}

type Reply<T> = std::sync::mpsc::Sender<Result<T>>;

pub struct Store {
    tx: Sender<Cmd>,
    path: PathBuf,
    readers: Mutex<Vec<Connection>>,
    handle: Option<thread::JoinHandle<()>>,
}

pub struct Reader<'a> {
    conn: Option<Connection>,
    pool: &'a Mutex<Vec<Connection>>,
}

impl std::ops::Deref for Reader<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("a live reader always holds its connection")
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        let conn = match self.conn.take() {
            Some(c) => c,
            None => return,
        };
        if let Ok(mut pool) = self.pool.lock() {
            if pool.len() < READER_POOL {
                pool.push(conn);
            }
        }
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        schema::migrate(&conn)?;

        let (tx, rx) = channel::<Cmd>();
        let handle = thread::Builder::new()
            .name("studio-store-writer".into())
            .spawn(move || {
                let mut seq_by_run: HashMap<String, u64> = HashMap::new();
                if let Ok(mut stmt) =
                    conn.prepare("SELECT run, MAX(seq) FROM events GROUP BY run")
                {
                    if let Ok(rows) = stmt.query_map([], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
                    }) {
                        for row in rows.flatten() {
                            seq_by_run.insert(row.0, row.1);
                        }
                    }
                }

                let _ = fold_the_wal_back_in(&conn);

                for cmd in rx {
                    match cmd {
                        Cmd::Shutdown => break,
                        other => handle_cmd(&conn, &mut seq_by_run, other),
                    }
                }
                let _ = fold_the_wal_back_in(&conn);
            })
            .expect("spawn store writer");

        Ok(Self { tx, path, readers: Mutex::new(Vec::new()), handle: Some(handle) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn checkpoint(&self) -> Result<u64> {
        self.send(Cmd::Checkpoint)
    }

    fn send<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
        let (rtx, rrx) = channel();
        self.tx.send(make(rtx)).map_err(|_| StoreError::WriterGone)?;
        rrx.recv().map_err(|_| StoreError::WriterGone)?
    }

    pub fn upsert_role(&self, role: RoleRow) -> Result<()> {
        self.send(|r| Cmd::UpsertRole(role, r))
    }

    pub fn insert_project(&self, project: ProjectRow, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::InsertProject(project, ts, r))
    }

    pub fn touch_project(&self, id: impl Into<String>, ts: impl Into<String>) -> Result<()> {
        let (id, ts) = (id.into(), ts.into());
        self.send(|r| Cmd::TouchProject(id, ts, r))
    }

    pub fn projects(&self) -> Result<Vec<ProjectRow>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, root, engine, git FROM projects
             WHERE forgotten_ts IS NULL
             ORDER BY last_used DESC NULLS LAST, created_ts DESC",
        )?;
        let rows = stmt.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn forget_project(&self, id: impl Into<String>, ts: impl Into<String>) -> Result<bool> {
        let (id, ts) = (id.into(), ts.into());
        self.send(|r| Cmd::ForgetProject(id, ts, r))
    }

    pub fn purge_project(&self, id: impl Into<String>) -> Result<Purged> {
        let id = id.into();
        self.send(|r| Cmd::PurgeProject(id, r))
    }

    pub fn project(&self, id: &str) -> Result<Option<ProjectRow>> {
        let conn = self.reader()?;
        let mut stmt = conn
            .prepare("SELECT id, name, root, engine, git FROM projects WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], project_from_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn insert_task(&self, task: TaskRow, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::InsertTask(task, ts, r))
    }

    pub fn update_task_state(
        &self,
        task_id: impl Into<String>,
        state: WorkerState,
        outcome: Option<Outcome>,
        ts: impl Into<String>,
    ) -> Result<()> {
        let (id, ts) = (task_id.into(), ts.into());
        self.send(|r| Cmd::UpdateTaskState(id, state, outcome, ts, r))
    }

    pub fn insert_session(&self, s: SessionRow, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::InsertSession(s, ts, r))
    }

    pub fn append_event(
        &self,
        run: impl Into<String>,
        ts: impl Into<String>,
        actor: impl Into<String>,
        event_type: EventType,
        scene: Scene,
        data: serde_json::Value,
    ) -> Result<Envelope> {
        let (run, ts, actor) = (run.into(), ts.into(), actor.into());
        self.send(|reply| Cmd::AppendEvent { run, ts, actor, event_type, scene, data, reply })
    }

    pub fn insert_capsule(&self, c: CapsuleRow, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::InsertCapsule(c, ts, r))
    }

    pub fn insert_decision(&self, d: DecisionRow, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::InsertDecision(d, ts, r))
    }

    pub fn search_decisions(&self, query: &str, limit: usize) -> Result<Vec<DecisionRow>> {
        let cleaned = sanitize_fts(query);
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT d.id, d.title, d.claim, d.rationale, d.origin_capsule, d.supersedes
             FROM decisions_fts f
             JOIN decisions d ON d.rowid = f.rowid
             WHERE decisions_fts MATCH ?1
               AND d.id NOT IN (SELECT supersedes FROM decisions WHERE supersedes IS NOT NULL)
             ORDER BY bm25(decisions_fts)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![cleaned, limit as i64], |r| {
            Ok(DecisionRow {
                id: r.get(0)?,
                title: r.get(1)?,
                claim: r.get(2)?,
                rationale: r.get(3)?,
                origin_capsule: r.get(4)?,
                supersedes: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn capsules_for_task(&self, task: &str) -> Result<Vec<CapsuleRow>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, task, kind, from_role, outcome, rendered_tokens, truncated, body_json
             FROM capsules WHERE task = ?1 ORDER BY created_ts",
        )?;
        let rows = stmt.query_map(params![task], |r| {
            Ok(CapsuleRow {
                id: r.get(0)?,
                task: r.get(1)?,
                kind: r.get(2)?,
                from_role: r.get(3)?,
                outcome: r.get(4)?,
                rendered_tokens: r.get::<_, i64>(5)? as usize,
                truncated: r.get::<_, i64>(6)? != 0,
                body_json: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn record_usage(&self, entry: LedgerEntry, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::RecordUsage(entry, ts, r))
    }

    fn reader(&self) -> Result<Reader<'_>> {
        let pooled = self.readers.lock().ok().and_then(|mut p| p.pop());
        let conn = match pooled {
            Some(conn) => conn,
            None => {
                let conn = Connection::open_with_flags(
                    &self.path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
                )?;
                conn.busy_timeout(std::time::Duration::from_millis(
                    schema::BUSY_TIMEOUT_MS as u64,
                ))?;
                conn
            }
        };
        Ok(Reader { conn: Some(conn), pool: &self.readers })
    }

    pub fn head_seq(&self, run: &str) -> Result<u64> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached("SELECT MAX(seq) FROM events WHERE run = ?1")?;
        let head: Option<i64> = stmt.query_row(params![run], |r| r.get(0))?;
        Ok(head.unwrap_or(0) as u64)
    }

    pub fn events_since(&self, run: &str, since_seq: u64) -> Result<Vec<Envelope>> {
        self.events_between(run, since_seq, u64::MAX)
    }

    pub fn events_between(&self, run: &str, since_seq: u64, limit: u64) -> Result<Vec<Envelope>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT run, seq, ts, actor, type, scene_json, data_json
             FROM events WHERE run = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![run, since_seq as i64, limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (run, seq, ts, actor, ty, scene, data) = row?;
            let event_type = match wire_index().get(ty.as_str()).copied() {
                Some(t) => t,
                None => serde_json::from_str::<EventType>(&format!("\"{ty}\""))?,
            };
            out.push(Envelope::new(
                seq as u64,
                ts,
                run,
                actor,
                serde_json::from_str::<Scene>(&scene)?,
                event_type,
                serde_json::from_str(&data)?,
            ));
        }
        Ok(out)
    }

    pub fn run_spend(&self, run: &str) -> Result<Spend> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT l.task, l.estimate, l.input, l.output, l.cost_usd
             FROM token_ledger l
             JOIN tasks t ON t.id = l.task
             WHERE t.run = ?1",
        )?;
        let rows = stmt.query_map(params![run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, i64>(3)? as u64,
                r.get::<_, f64>(4)?,
            ))
        })?;

        let mut finals: HashMap<String, (u64, f64)> = HashMap::new();
        let mut estimates: HashMap<String, (u64, f64)> = HashMap::new();
        for row in rows {
            let (task, is_estimate, input, output, usd) = row?;
            let bucket = if is_estimate { &mut estimates } else { &mut finals };
            let e = bucket.entry(task).or_insert((0, 0.0));
            e.0 += input + output;
            e.1 += usd;
        }

        let mut spend = Spend::default();
        for (task, (tokens, usd)) in &finals {
            spend.tokens += tokens;
            spend.usd += usd;
            estimates.remove(task);
        }
        for (tokens, usd) in estimates.values() {
            spend.tokens += tokens;
            spend.usd += usd;
        }
        Ok(spend)
    }

    pub fn cache_health(&self, since_ts: &str) -> Result<Vec<CacheHealth>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT role, prefix_hash, SUM(cache_read), SUM(cache_creation)
             FROM token_ledger
             WHERE estimate = 0 AND ts >= ?1
             GROUP BY role, prefix_hash
             ORDER BY role, prefix_hash",
        )?;
        let rows = stmt.query_map(params![since_ts], |r| {
            Ok(CacheHealth {
                role: r.get(0)?,
                prefix_hash: r.get(1)?,
                cache_read: r.get::<_, i64>(2)? as u64,
                cache_creation: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn unfinished_tasks(&self) -> Result<Vec<(TaskRow, Option<SessionRow>)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT t.id, t.run, t.role, t.parent_task, t.workflow_node, t.state,
                    s.session_id, s.prefix_hash, s.forked_from, s.jsonl_path
             FROM tasks t
             LEFT JOIN sessions s ON s.task = t.id
             WHERE t.outcome IS NULL
             ORDER BY t.created_ts",
        )?;
        let rows = stmt.query_map([], |r| {
            let state: String = r.get(5)?;
            let task = TaskRow {
                id: r.get(0)?,
                run: r.get(1)?,
                role: r.get(2)?,
                parent_task: r.get(3)?,
                workflow_node: r.get(4)?,
                state: serde_json::from_str(&format!("\"{state}\"")).unwrap_or(WorkerState::Queued),
                outcome: None,
                project: None,
            };
            let session = match r.get::<_, Option<String>>(6)? {
                Some(session_id) => Some(SessionRow {
                    session_id,
                    task: task.id.clone(),
                    prefix_hash: r.get(7)?,
                    forked_from: r.get(8)?,
                    jsonl_path: r.get(9)?,
                }),
                None => None,
            };
            Ok((task, session))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Ok(mut pool) = self.readers.lock() {
            pool.clear();
        }
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn sanitize_fts(q: &str) -> String {
    let terms: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    terms.join(" OR ")
}

fn tag(t: EventType) -> String {
    t.wire_name().to_string()
}

fn state_tag(s: WorkerState) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn outcome_tag(o: Outcome) -> String {
    serde_json::to_value(o)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn purge_project(conn: &Connection, id: &str) -> Result<Purged> {
    conn.execute_batch("PRAGMA defer_foreign_keys = ON; BEGIN IMMEDIATE")?;
    let purged = cascade(conn, id);
    match purged {
        Ok(counted) => {
            conn.execute_batch("COMMIT")?;
            Ok(counted)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn cascade(conn: &Connection, id: &str) -> Result<Purged> {
    let tasks: Vec<String> = conn
        .prepare("SELECT id FROM tasks WHERE project = ?1")?
        .query_map(params![id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let runs: Vec<String> = conn
        .prepare(
            "SELECT DISTINCT run FROM tasks WHERE project = ?1
             AND run NOT IN (SELECT run FROM tasks WHERE project IS NOT ?1)",
        )?
        .query_map(params![id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut counted = Purged::default();
    for task in &tasks {
        conn.execute(
            "DELETE FROM artifacts WHERE capsule IN (SELECT id FROM capsules WHERE task = ?1)",
            params![task],
        )?;
        conn.execute(
            "DELETE FROM decisions WHERE origin_capsule IN
             (SELECT id FROM capsules WHERE task = ?1)",
            params![task],
        )?;
        counted.capsules += conn.execute("DELETE FROM capsules WHERE task = ?1", params![task])?;
        counted.ledger_rows +=
            conn.execute("DELETE FROM token_ledger WHERE task = ?1", params![task])?;
        conn.execute("DELETE FROM sessions WHERE task = ?1", params![task])?;
    }

    for run in &runs {
        counted.events += conn.execute("DELETE FROM events WHERE run = ?1", params![run])?;
        conn.execute(
            "DELETE FROM budgets WHERE scope_kind = 'run' AND scope_id = ?1",
            params![run],
        )?;
    }

    counted.tasks = conn.execute("DELETE FROM tasks WHERE project = ?1", params![id])?;
    conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    Ok(counted)
}

fn project_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        id: r.get(0)?,
        name: r.get(1)?,
        root: r.get(2)?,
        engine: r.get(3)?,
        git: r.get::<_, i64>(4)? != 0,
    })
}

pub fn fold_the_wal_back_in(conn: &Connection) -> rusqlite::Result<u64> {
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
        r.get::<_, i64>(1).map(|pages| pages.max(0) as u64)
    })
}

fn handle_cmd(conn: &Connection, seq_by_run: &mut HashMap<String, u64>, cmd: Cmd) {
    match cmd {
        Cmd::Shutdown => {}

        Cmd::Checkpoint(reply) => {
            let _ = reply.send(fold_the_wal_back_in(conn).map_err(StoreError::from));
        }

        Cmd::InsertProject(p, ts, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO projects (id, name, root, engine, git, created_ts, last_used)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params![p.id, p.name, p.root, p.engine, p.git as i64, ts],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::TouchProject(id, ts, reply) => {
            let res = conn
                .execute("UPDATE projects SET last_used = ?2 WHERE id = ?1", params![id, ts])
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::ForgetProject(id, ts, reply) => {
            let res = conn
                .execute(
                    "UPDATE projects SET forgotten_ts = ?2
                     WHERE id = ?1 AND forgotten_ts IS NULL",
                    params![id, ts],
                )
                .map(|changed| changed > 0)
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::PurgeProject(id, reply) => {
            let _ = reply.send(purge_project(conn, &id));
        }

        Cmd::UpsertRole(role, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO roles (id, tier, department, model, effort, escalates_to)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET
                       tier=excluded.tier, department=excluded.department,
                       model=excluded.model, effort=excluded.effort,
                       escalates_to=excluded.escalates_to",
                    params![
                        role.id,
                        role.tier as i64,
                        role.department,
                        role.model,
                        role.effort,
                        role.escalates_to
                    ],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::InsertCapsule(c, ts, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO capsules
                       (id, task, kind, from_role, outcome, rendered_tokens, truncated, body_json, created_ts)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        c.id, c.task, c.kind, c.from_role, c.outcome,
                        c.rendered_tokens as i64, c.truncated as i64, c.body_json, ts
                    ],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::InsertDecision(d, ts, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO decisions
                       (id, title, claim, rationale, origin_capsule, supersedes, created_ts)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![d.id, d.title, d.claim, d.rationale, d.origin_capsule, d.supersedes, ts],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::InsertTask(task, ts, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO tasks
                       (id, run, role, parent_task, workflow_node, state, outcome, created_ts, updated_ts, project)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7, ?8)",
                    params![
                        task.id,
                        task.run,
                        task.role,
                        task.parent_task,
                        task.workflow_node,
                        state_tag(task.state),
                        ts,
                        task.project
                    ],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::UpdateTaskState(id, state, outcome, ts, reply) => {
            let res = conn
                .execute(
                    "UPDATE tasks SET state = ?2, outcome = ?3, updated_ts = ?4 WHERE id = ?1",
                    params![id, state_tag(state), outcome.map(outcome_tag), ts],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::InsertSession(s, ts, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO sessions
                       (session_id, task, prefix_hash, forked_from, jsonl_path, created_ts)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![s.session_id, s.task, s.prefix_hash, s.forked_from, s.jsonl_path, ts],
                )
                .map(|_| ())
                .map_err(StoreError::from);
            let _ = reply.send(res);
        }

        Cmd::AppendEvent { run, ts, actor, event_type, scene, data, reply } => {
            let next = seq_by_run.get(&run).copied().unwrap_or(0) + 1;
            let res = (|| -> Result<Envelope> {
                let scene_json = serde_json::to_string(&scene)?;
                let data_json = serde_json::to_string(&data)?;
                conn.execute(
                    "INSERT INTO events (run, seq, ts, actor, type, scene_json, data_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![run, next as i64, ts, actor, tag(event_type), scene_json, data_json],
                )?;
                Ok(Envelope::new(next, ts, run.clone(), actor, scene, event_type, data))
            })();

            if res.is_ok() {
                seq_by_run.insert(run, next);
            }
            let _ = reply.send(res);
        }

        Cmd::RecordUsage(e, ts, reply) => {
            let res = (|| -> Result<()> {
                if e.estimate {
                    conn.execute(
                        "INSERT INTO token_ledger
                           (task, role, prefix_hash, estimate, input, output,
                            cache_read, cache_creation, cost_usd, model, ts)
                         VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                         ON CONFLICT(task) WHERE estimate = 1 DO UPDATE SET
                           input=excluded.input, output=excluded.output,
                           cache_read=excluded.cache_read,
                           cache_creation=excluded.cache_creation,
                           cost_usd=excluded.cost_usd, ts=excluded.ts",
                        params![
                            e.task,
                            e.role,
                            e.prefix_hash,
                            e.usage.input as i64,
                            e.usage.output as i64,
                            e.usage.cache_read as i64,
                            e.usage.cache_creation as i64,
                            e.cost_usd,
                            e.model,
                            ts
                        ],
                    )?;
                } else {
                    conn.execute(
                        "INSERT INTO token_ledger
                           (task, role, prefix_hash, estimate, input, output,
                            cache_read, cache_creation, cost_usd, model, ts)
                         VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            e.task,
                            e.role,
                            e.prefix_hash,
                            e.usage.input as i64,
                            e.usage.output as i64,
                            e.usage.cache_read as i64,
                            e.usage.cache_creation as i64,
                            e.cost_usd,
                            e.model,
                            ts
                        ],
                    )?;
                    conn.execute(
                        "DELETE FROM token_ledger WHERE task = ?1 AND estimate = 1",
                        params![e.task],
                    )?;
                }
                Ok(())
            })();
            let _ = reply.send(res);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_events::Usage;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("studio-state.db")).unwrap();
        s.upsert_role(RoleRow {
            id: "gameplay_engineer".into(),
            tier: 3,
            department: "engineering".into(),
            model: "opus".into(),
            effort: "high".into(),
            escalates_to: None,
        })
        .unwrap();
        (dir, s)
    }

    fn task(s: &Store, id: &str, run: &str) {
        s.insert_task(
            TaskRow {
                id: id.into(),
                run: run.into(),
                role: "gameplay_engineer".into(),
                parent_task: None,
                workflow_node: None,
                state: WorkerState::Queued,
                outcome: None,
                project: None,
            },
            "2026-07-20T00:00:00Z",
        )
        .unwrap();
    }

    fn game(s: &Store, id: &str) {
        s.insert_project(
            ProjectRow {
                id: id.into(),
                name: id.into(),
                root: format!("C:/games/{id}"),
                engine: "godot".into(),
                git: false,
            },
            "2026-07-20T00:00:00Z",
        )
        .unwrap();
    }

    fn task_of(s: &Store, id: &str, run: &str, project: &str) {
        s.insert_task(
            TaskRow {
                id: id.into(),
                run: run.into(),
                role: "gameplay_engineer".into(),
                parent_task: None,
                workflow_node: None,
                state: WorkerState::Queued,
                outcome: None,
                project: Some(project.into()),
            },
            "2026-07-20T00:00:00Z",
        )
        .unwrap();
    }

    #[test]
    fn a_forgotten_game_leaves_the_list_with_everything_it_ran_still_on_file() {
        let (_d, s) = store();
        game(&s, "proj_a");
        task_of(&s, "task_a", "run_a", "proj_a");
        s.append_event("run_a", "ts", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
            .unwrap();

        assert!(s.forget_project("proj_a", "2026-07-29T00:00:00Z").unwrap());
        assert!(s.projects().unwrap().is_empty(), "the list is what forgetting changes");
        assert!(
            s.project("proj_a").unwrap().is_some(),
            "a run that resumes still has to find the project it belonged to"
        );
        assert_eq!(s.events_since("run_a", 0).unwrap().len(), 1);
        assert!(
            !s.forget_project("proj_a", "2026-07-29T00:00:00Z").unwrap(),
            "forgetting twice is not an error to shout about, it is a no-op to report"
        );
    }

    #[test]
    fn erasing_a_game_takes_its_whole_history_and_leaves_its_neighbour_untouched() {
        let (_d, s) = store();
        game(&s, "proj_gone");
        game(&s, "proj_kept");
        task_of(&s, "task_gone", "run_gone", "proj_gone");
        task_of(&s, "task_kept", "run_kept", "proj_kept");

        for task in ["task_gone", "task_kept"] {
            s.insert_capsule(
                CapsuleRow {
                    id: format!("cap_{task}"),
                    task: task.into(),
                    kind: "task_return".into(),
                    from_role: "gameplay_engineer".into(),
                    outcome: "done".into(),
                    rendered_tokens: 10,
                    truncated: false,
                    body_json: "{}".into(),
                },
                "ts",
            )
            .unwrap();
            s.record_usage(ledger(task, false, 1, 1, 0, 0, 0.1), "ts").unwrap();
        }
        for run in ["run_gone", "run_kept"] {
            s.append_event(run, "ts", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }

        let gone = s.purge_project("proj_gone").unwrap();
        assert_eq!(gone.tasks, 1);
        assert_eq!(gone.events, 1);
        assert_eq!(gone.capsules, 1);
        assert_eq!(gone.ledger_rows, 1);

        assert_eq!(s.projects().unwrap().len(), 1, "only the neighbour is left");
        assert!(s.events_since("run_gone", 0).unwrap().is_empty());
        assert_eq!(
            s.events_since("run_kept", 0).unwrap().len(),
            1,
            "erasing one game must not touch the events of another"
        );
        assert_eq!(s.capsules_for_task("task_kept").unwrap().len(), 1);
        assert!(s.capsules_for_task("task_gone").unwrap().is_empty());
    }

    fn ledger(task: &str, estimate: bool, input: u64, output: u64, read: u64, write: u64, usd: f64) -> LedgerEntry {
        LedgerEntry {
            task: task.into(),
            role: "gameplay_engineer".into(),
            prefix_hash: "b3:deadbeef".into(),
            estimate,
            usage: Usage { input, output, cache_read: read, cache_creation: write },
            cost_usd: usd,
            model: "opus".into(),
        }
    }

    fn wal_bytes(path: &Path) -> u64 {
        let wal = path.with_extension("db-wal");
        std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0)
    }

    #[test]
    fn a_checkpoint_folds_the_write_ahead_log_back_into_the_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let store = Store::open(&path).unwrap();

        for i in 0..2000 {
            store
                .append_event(
                    "run_wal",
                    "2026-07-26T00:00:00Z",
                    "daemon",
                    EventType::WorkerSpawned,
                    Scene::daemon(),
                    serde_json::json!({"i": i, "padding": "x".repeat(200)}),
                )
                .unwrap();
        }

        let before = wal_bytes(&path);
        assert!(before > 0, "these writes are supposed to have gone through a wal");

        store.checkpoint().unwrap();
        assert_eq!(
            wal_bytes(&path),
            0,
            "the daemon is killed rather than closed, so a wal that is only folded in on a clean \
             close is a wal that survives every run: {before} bytes were left behind"
        );

        assert_eq!(
            store.events_between("run_wal", 0, 5000).unwrap().len(),
            2000,
            "a checkpoint must move the events into the file, not drop them"
        );
    }

    #[test]
    fn a_store_that_is_dropped_leaves_no_write_ahead_log_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let store = Store::open(&path).unwrap();
            for i in 0..500 {
                store
                    .append_event(
                        "run_drop",
                        "2026-07-26T00:00:00Z",
                        "daemon",
                        EventType::WorkerSpawned,
                        Scene::daemon(),
                        serde_json::json!({"i": i}),
                    )
                    .unwrap();
            }
            let _ = store.events_between("run_drop", 0, 10).unwrap();
        }
        assert_eq!(wal_bytes(&path), 0);
    }

    #[test]
    fn a_freshly_migrated_store_puts_its_schema_in_the_file_rather_than_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let store = Store::open(&path).unwrap();
        store.checkpoint().unwrap();

        assert_eq!(
            wal_bytes(&path),
            0,
            "a daemon that starts and is then killed used to leave its whole schema in a wal \
             beside a database file that was still one page long"
        );
        assert!(
            std::fs::metadata(&path).unwrap().len() > 4096,
            "the schema has to have landed somewhere, and the file is the only place left"
        );
    }

    #[test]
    fn a_warm_reader_pool_does_not_stop_the_log_being_folded_back_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let store = Store::open(&path).unwrap();

        for i in 0..READER_POOL * 2 {
            store
                .append_event(
                    "run_pool",
                    "2026-07-26T00:00:00Z",
                    "daemon",
                    EventType::WorkerSpawned,
                    Scene::daemon(),
                    serde_json::json!({"i": i}),
                )
                .unwrap();
            let _ = store.events_between("run_pool", 0, 10).unwrap();
        }
        assert!(!store.readers.lock().unwrap().is_empty(), "the pool must be warm");

        store.checkpoint().unwrap();
        assert_eq!(
            wal_bytes(&path),
            0,
            "an idle pooled reader holds no snapshot, so throwing the pool away to checkpoint \
             would pay 0.9ms a read for nothing"
        );
        assert!(
            !store.readers.lock().unwrap().is_empty(),
            "the checkpoint must leave the pool it did not need to clear"
        );
    }

    #[test]
    fn migration_is_idempotent_and_sets_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let _s = Store::open(&path).unwrap();
        }
        let s = Store::open(&path).unwrap();
        let conn = s.reader().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let v: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn event_seq_is_gap_free_and_per_run() {
        let (_d, s) = store();
        for _ in 0..5 {
            s.append_event("run_a", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }
        for _ in 0..3 {
            s.append_event("run_b", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }

        let a = s.events_since("run_a", 0).unwrap();
        let b = s.events_since("run_b", 0).unwrap();
        assert_eq!(a.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
        assert_eq!(b.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn seq_resumes_after_reopen_without_a_gap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let s = Store::open(&path).unwrap();
            for _ in 0..3 {
                s.append_event("run_a", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                    .unwrap();
            }
        }
        let s = Store::open(&path).unwrap();
        let e = s
            .append_event("run_a", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
            .unwrap();
        assert_eq!(e.seq, 4);
    }

    #[test]
    fn events_since_supports_resume() {
        let (_d, s) = store();
        for _ in 0..6 {
            s.append_event("r", "t", "daemon", EventType::TokenUsage, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }
        let tail = s.events_since("r", 4).unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].seq, 5);
    }

    #[test]
    fn the_head_of_a_run_is_readable_without_reading_the_run() {
        let (_d, s) = store();
        for _ in 0..40 {
            s.append_event("r", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }
        for _ in 0..7 {
            s.append_event("other", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }
        assert_eq!(s.head_seq("r").unwrap(), 40);
        assert_eq!(s.head_seq("other").unwrap(), 7);
    }

    #[test]
    fn the_head_of_a_run_nobody_has_written_is_zero_rather_than_an_error() {
        let (_d, s) = store();
        assert_eq!(s.head_seq("never_ran").unwrap(), 0);
    }

    #[test]
    fn a_bounded_read_stops_at_the_limit_it_was_given() {
        let (_d, s) = store();
        for _ in 0..100 {
            s.append_event("r", "t", "daemon", EventType::ToolCall, Scene::daemon(), serde_json::json!({}))
                .unwrap();
        }
        let page = s.events_between("r", 20, 10).unwrap();
        assert_eq!(page.len(), 10);
        assert_eq!(page.first().unwrap().seq, 21);
        assert_eq!(page.last().unwrap().seq, 30);
        assert_eq!(
            s.events_since("r", 20).unwrap().len(),
            80,
            "an unbounded read still returns the whole tail"
        );
    }

    #[test]
    fn readers_are_reused_across_calls_without_changing_what_they_answer() {
        let (_d, s) = store();
        for _ in 0..5 {
            s.append_event("r", "t", "daemon", EventType::CacheHit, Scene::daemon(), serde_json::json!({"n": 1}))
                .unwrap();
        }
        for _ in 0..20 {
            assert_eq!(s.events_since("r", 0).unwrap().len(), 5);
            assert_eq!(s.head_seq("r").unwrap(), 5);
        }
        s.append_event("r", "t", "daemon", EventType::CacheHit, Scene::daemon(), serde_json::json!({"n": 2}))
            .unwrap();
        assert_eq!(
            s.head_seq("r").unwrap(),
            6,
            "a pooled reader must still see writes that landed after it was parked"
        );
    }

    #[test]
    fn event_round_trips_through_sqlite() {
        let (_d, s) = store();
        let sent = s
            .append_event(
                "r",
                "2026-07-20T09:12:44.118Z",
                "gameplay_engineer#7",
                EventType::CacheHit,
                Scene::desk("engineering", "gameplay_engineer#7"),
                serde_json::json!({"cache_read": 8867, "cache_creation": 0}),
            )
            .unwrap();
        let back = s.events_since("r", 0).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], sent);
        assert_eq!(back[0].data["cache_read"], 8867);
    }

    #[test]
    fn a_final_ledger_row_supersedes_the_live_estimate() {
        let (_d, s) = store();
        task(&s, "task_1", "r");

        s.record_usage(ledger("task_1", true, 1000, 100, 0, 0, 0.05), "t1").unwrap();
        assert_eq!(s.run_spend("r").unwrap().tokens, 1100);

        s.record_usage(ledger("task_1", true, 2000, 300, 0, 0, 0.09), "t2").unwrap();
        assert_eq!(
            s.run_spend("r").unwrap().tokens,
            2300,
            "the estimate should be upserted, not accumulated"
        );

        s.record_usage(ledger("task_1", false, 2400, 400, 0, 0, 0.11), "t3").unwrap();
        let spend = s.run_spend("r").unwrap();
        assert_eq!(spend.tokens, 2800, "the final row must supersede the estimate");
        assert!((spend.usd - 0.11).abs() < 1e-9);
    }

    #[test]
    fn spend_mixes_final_and_in_flight_tasks() {
        let (_d, s) = store();
        task(&s, "done", "r");
        task(&s, "live", "r");
        s.record_usage(ledger("done", false, 1000, 200, 0, 0, 0.06), "t").unwrap();
        s.record_usage(ledger("live", true, 500, 50, 0, 0, 0.03), "t").unwrap();
        assert_eq!(s.run_spend("r").unwrap().tokens, 1750);
    }

    #[test]
    fn spend_is_scoped_to_one_run() {
        let (_d, s) = store();
        task(&s, "t_a", "run_a");
        task(&s, "t_b", "run_b");
        s.record_usage(ledger("t_a", false, 100, 10, 0, 0, 0.01), "t").unwrap();
        s.record_usage(ledger("t_b", false, 900, 90, 0, 0, 0.09), "t").unwrap();
        assert_eq!(s.run_spend("run_a").unwrap().tokens, 110);
    }

    #[test]
    fn cache_health_computes_the_hit_ratio_from_final_rows_only() {
        let (_d, s) = store();
        task(&s, "cold", "r");
        task(&s, "warm", "r");
        s.record_usage(ledger("cold", false, 2, 4, 0, 8867, 0.0888), "2026-07-20T10:00:00Z").unwrap();
        s.record_usage(ledger("warm", false, 2, 4, 8867, 0, 0.0051), "2026-07-20T10:01:00Z").unwrap();
        s.record_usage(ledger("warm", true, 9999, 0, 0, 0, 9.0), "2026-07-20T10:02:00Z").unwrap();

        let health = s.cache_health("2026-07-20T00:00:00Z").unwrap();
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].cache_read, 8867);
        assert_eq!(health[0].cache_creation, 8867);
        assert!((health[0].hit_ratio().unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cache_health_reports_none_when_nothing_was_measured() {
        let h = CacheHealth {
            role: "r".into(),
            prefix_hash: "p".into(),
            cache_read: 0,
            cache_creation: 0,
        };
        assert!(h.hit_ratio().is_none());
    }

    #[test]
    fn crash_recovery_lists_unfinished_tasks_with_their_sessions() {
        let (_d, s) = store();
        task(&s, "live", "r");
        task(&s, "done", "r");
        s.insert_session(
            SessionRow {
                session_id: "sess_live".into(),
                task: "live".into(),
                prefix_hash: "b3:x".into(),
                forked_from: None,
                jsonl_path: "/p/live.jsonl".into(),
            },
            "t",
        )
        .unwrap();
        s.update_task_state("live", WorkerState::Streaming, None, "t").unwrap();
        s.update_task_state("done", WorkerState::Reaped, Some(Outcome::Completed), "t").unwrap();

        let open = s.unfinished_tasks().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].0.id, "live");
        assert_eq!(open[0].0.state, WorkerState::Streaming);
        assert_eq!(open[0].1.as_ref().unwrap().jsonl_path, "/p/live.jsonl");
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let (_d, s) = store();
        let err = s.insert_task(
            TaskRow {
                id: "orphan".into(),
                run: "r".into(),
                role: "no_such_role".into(),
                parent_task: None,
                workflow_node: None,
                state: WorkerState::Queued,
                outcome: None,
                project: None,
            },
            "t",
        );
        assert!(err.is_err());
    }
}

#[cfg(test)]
mod scale_probe {
    use super::*;
    use std::time::Instant;

    fn seeded(events: u64) -> (tempfile::TempDir, Store, std::time::Duration) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("studio-state.db")).unwrap();
        let data = serde_json::json!({
            "tool": "Read",
            "args_digest": "b3:6f1c2d9a4b7e0f3358c1d2e4a5b6c7d8",
            "bytes": 4096,
            "ok": true
        });
        let started = Instant::now();
        for i in 0..events {
            let ty = if i % 7 == 0 { EventType::ToolResult } else { EventType::ToolCall };
            s.append_event(
                "r",
                "2026-07-25T09:12:44.118Z",
                format!("gameplay_engineer#{}", i % 13),
                ty,
                Scene::desk("engineering", "gameplay_engineer#1"),
                data.clone(),
            )
            .unwrap();
        }
        let wrote = started.elapsed();
        (dir, s, wrote)
    }

    #[test]
    #[ignore]
    fn a_long_run_can_be_written_to_a_named_database_for_the_http_probes() {
        let path = match std::env::var("SEED_DB") {
            Ok(p) => p,
            Err(_) => {
                println!("set SEED_DB=<path> and SEED_EVENTS=<n> to seed a database");
                return;
            }
        };
        let count: u64 = std::env::var("SEED_EVENTS").ok().and_then(|v| v.parse().ok()).unwrap_or(50_000);
        let run = std::env::var("SEED_RUN").unwrap_or_else(|_| "probe_run".to_string());

        let s = Store::open(&path).unwrap();
        let data = serde_json::json!({
            "tool": "Read",
            "args_digest": "b3:6f1c2d9a4b7e0f3358c1d2e4a5b6c7d8",
            "bytes": 4096,
            "ok": true
        });
        let started = Instant::now();
        for i in 0..count {
            let ty = if i % 7 == 0 { EventType::ToolResult } else { EventType::ToolCall };
            s.append_event(
                &run,
                "2026-07-25T09:12:44.118Z",
                format!("gameplay_engineer#{}", i % 13),
                ty,
                Scene::desk("engineering", "gameplay_engineer#1"),
                data.clone(),
            )
            .unwrap();
        }
        println!(
            "wrote {count} events into run {run} at {path} in {:.0}ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }

    #[test]
    #[ignore]
    fn reading_a_long_run_costs_what_the_slice_costs_not_what_the_log_costs() {
        println!("events    write      whole log   tail of 100   head only");
        for n in [1_000u64, 10_000, 50_000] {
            let (_d, s, wrote) = seeded(n);

            let t = Instant::now();
            let all = s.events_since("r", 0).unwrap();
            let whole = t.elapsed();
            assert_eq!(all.len() as u64, n);

            let t = Instant::now();
            let tail = s.events_since("r", n - 100).unwrap();
            let slice = t.elapsed();
            assert_eq!(tail.len(), 100);

            let t = Instant::now();
            let head = s.head_seq("r").unwrap();
            let only_head = t.elapsed();
            assert_eq!(head, n);

            println!(
                "{n:<9} {:>7.1}ms {:>9.2}ms {:>11.3}ms {:>10.3}ms",
                wrote.as_secs_f64() * 1000.0,
                whole.as_secs_f64() * 1000.0,
                slice.as_secs_f64() * 1000.0,
                only_head.as_secs_f64() * 1000.0,
            );
        }
    }

    #[test]
    #[ignore]
    fn a_reconnecting_client_pays_for_the_backlog_it_asked_for() {
        let (_d, s, _) = seeded(50_000);
        let reconnects = 20;

        let t = Instant::now();
        for _ in 0..reconnects {
            let all = s.events_since("r", 0).unwrap();
            let head = all.last().map(|e| e.seq).unwrap_or(0);
            let _tail: Vec<Envelope> = all.into_iter().filter(|e| e.seq > head - 5).collect();
        }
        let whole_log_each_time = t.elapsed();

        let t = Instant::now();
        for _ in 0..reconnects {
            let head = s.head_seq("r").unwrap();
            let _tail = s.events_since("r", head - 5).unwrap();
        }
        let bounded = t.elapsed();

        println!(
            "{reconnects} reconnects to a 50000-event run: whole log each time {:.1}ms, bounded {:.1}ms",
            whole_log_each_time.as_secs_f64() * 1000.0,
            bounded.as_secs_f64() * 1000.0,
        );
    }
}

#[cfg(test)]
mod adr_tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("s.db")).unwrap();
        s.upsert_role(RoleRow {
            id: "systems_engineer".into(),
            tier: 2,
            department: "engineering".into(),
            model: "opus".into(),
            effort: "xhigh".into(),
            escalates_to: None,
        })
        .unwrap();
        s.insert_task(
            TaskRow {
                id: "t1".into(),
                run: "r".into(),
                role: "systems_engineer".into(),
                parent_task: None,
                workflow_node: None,
                state: WorkerState::Queued,
                outcome: None,
                project: None,
            },
            "ts",
        )
        .unwrap();
        (dir, s)
    }

    fn decision(id: &str, title: &str, claim: &str, supersedes: Option<&str>) -> DecisionRow {
        DecisionRow {
            id: id.into(),
            title: title.into(),
            claim: claim.into(),
            rationale: "because the ledger said so".into(),
            origin_capsule: None,
            supersedes: supersedes.map(str::to_string),
        }
    }

    #[test]
    fn a_decision_is_findable_by_full_text_search() {
        let (_d, s) = store();
        s.insert_decision(
            decision("adr_1", "Dash implementation", "Dash is a state machine", None),
            "ts",
        )
        .unwrap();
        s.insert_decision(
            decision("adr_2", "Audio bus layout", "Audio routes through a single bus", None),
            "ts",
        )
        .unwrap();

        let hits = s.search_decisions("dash state machine", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "adr_1");
    }

    #[test]
    fn the_fts_index_is_kept_in_sync_by_triggers() {
        let (_d, s) = store();
        s.insert_decision(decision("adr_1", "Netcode", "Rollback netcode is used", None), "ts")
            .unwrap();
        assert_eq!(s.search_decisions("rollback", 5).unwrap().len(), 1);
    }

    #[test]
    fn a_superseded_decision_stops_being_pushed() {
        let (_d, s) = store();
        s.insert_decision(decision("adr_1", "Dash", "Dash is a coroutine", None), "ts")
            .unwrap();
        s.insert_decision(
            decision("adr_2", "Dash", "Dash is a state machine", Some("adr_1")),
            "ts",
        )
        .unwrap();

        let hits = s.search_decisions("dash", 5).unwrap();
        assert_eq!(hits.len(), 1, "the superseded ADR must not resurface");
        assert_eq!(hits[0].id, "adr_2");
    }

    #[test]
    fn the_push_is_capped_at_the_requested_top_n() {
        let (_d, s) = store();
        for i in 0..12 {
            s.insert_decision(
                decision(&format!("adr_{i}"), "Physics", "Physics runs fixed step", None),
                "ts",
            )
            .unwrap();
        }
        assert_eq!(s.search_decisions("physics", 5).unwrap().len(), 5);
        assert_eq!(s.search_decisions("physics", 3).unwrap().len(), 3);
    }

    #[test]
    fn a_query_with_fts_syntax_does_not_blow_up() {
        let (_d, s) = store();
        s.insert_decision(decision("adr_1", "Dash", "Dash is a state machine", None), "ts")
            .unwrap();
        for q in ["dash AND (", "\"unterminated", "NEAR/", "*", "  ", "a"] {
            assert!(s.search_decisions(q, 5).is_ok(), "query {q:?} must not error");
        }
    }

    #[test]
    fn a_capsule_round_trips_with_its_render_metadata() {
        let (_d, s) = store();
        s.insert_capsule(
            CapsuleRow {
                id: "cap_1".into(),
                task: "t1".into(),
                kind: "task_return".into(),
                from_role: "systems_engineer".into(),
                outcome: "done".into(),
                rendered_tokens: 812,
                truncated: true,
                body_json: r#"{"v":1}"#.into(),
            },
            "ts",
        )
        .unwrap();

        let back = s.capsules_for_task("t1").unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].rendered_tokens, 812);
        assert!(back[0].truncated);
    }

    #[test]
    fn migrating_a_v1_database_backfills_the_fts_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            )
            .unwrap();
            conn.execute_batch(crate::schema::v1_for_test()).unwrap();
            conn.execute(
                "INSERT INTO decisions (id,title,claim,rationale,created_ts)
                 VALUES ('old','Legacy','Legacy claim about shaders','r','ts')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO meta (key,value) VALUES ('schema_version','1')",
                [],
            )
            .unwrap();
        }

        let s = Store::open(&path).unwrap();
        let hits = s.search_decisions("shaders", 5).unwrap();
        assert_eq!(hits.len(), 1, "rows written before V2 must be searchable after it");
        assert_eq!(hits[0].id, "old");
    }
}
