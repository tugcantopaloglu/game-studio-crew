mod schema;

pub use schema::SCHEMA_VERSION;

use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use studio_events::{Envelope, EventType, Outcome, Scene, Usage, WorkerState};

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
    Shutdown,
}

type Reply<T> = std::sync::mpsc::Sender<Result<T>>;

pub struct Store {
    tx: Sender<Cmd>,
    path: PathBuf,
    handle: Option<thread::JoinHandle<()>>,
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

                for cmd in rx {
                    match cmd {
                        Cmd::Shutdown => break,
                        other => handle_cmd(&conn, &mut seq_by_run, other),
                    }
                }
            })
            .expect("spawn store writer");

        Ok(Self { tx, path, handle: Some(handle) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn send<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
        let (rtx, rrx) = channel();
        self.tx.send(make(rtx)).map_err(|_| StoreError::WriterGone)?;
        rrx.recv().map_err(|_| StoreError::WriterGone)?
    }

    pub fn upsert_role(&self, role: RoleRow) -> Result<()> {
        self.send(|r| Cmd::UpsertRole(role, r))
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

    pub fn record_usage(&self, entry: LedgerEntry, ts: impl Into<String>) -> Result<()> {
        let ts = ts.into();
        self.send(|r| Cmd::RecordUsage(entry, ts, r))
    }

    fn reader(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        Ok(conn)
    }

    pub fn events_since(&self, run: &str, since_seq: u64) -> Result<Vec<Envelope>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT run, seq, ts, actor, type, scene_json, data_json
             FROM events WHERE run = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![run, since_seq as i64], |r| {
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
            out.push(Envelope::new(
                seq as u64,
                ts,
                run,
                actor,
                serde_json::from_str::<Scene>(&scene)?,
                serde_json::from_str::<EventType>(&format!("\"{ty}\""))?,
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
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
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

fn handle_cmd(conn: &Connection, seq_by_run: &mut HashMap<String, u64>, cmd: Cmd) {
    match cmd {
        Cmd::Shutdown => {}

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

        Cmd::InsertTask(task, ts, reply) => {
            let res = conn
                .execute(
                    "INSERT INTO tasks
                       (id, run, role, parent_task, workflow_node, state, outcome, created_ts, updated_ts)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7)",
                    params![
                        task.id,
                        task.run,
                        task.role,
                        task.parent_task,
                        task.workflow_node,
                        state_tag(task.state),
                        ts
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
            },
            "2026-07-20T00:00:00Z",
        )
        .unwrap();
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
            },
            "t",
        );
        assert!(err.is_err());
    }
}
