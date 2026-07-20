use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 2;

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current >= SCHEMA_VERSION {
        return Ok(());
    }

    if current < 1 {
        conn.execute_batch(V1)?;
    }
    if current < 2 {
        conn.execute_batch(V2)?;
    }

    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

const V1: &str = r#"
CREATE TABLE roles (
  id            TEXT PRIMARY KEY,
  tier          INTEGER NOT NULL,
  department    TEXT NOT NULL,
  model         TEXT NOT NULL,
  effort        TEXT NOT NULL,
  escalates_to  TEXT REFERENCES roles(id)
);

CREATE TABLE tasks (
  id            TEXT PRIMARY KEY,
  run           TEXT NOT NULL,
  role          TEXT NOT NULL REFERENCES roles(id),
  parent_task   TEXT REFERENCES tasks(id),
  workflow_node TEXT,
  state         TEXT NOT NULL,
  outcome       TEXT,
  created_ts    TEXT NOT NULL,
  updated_ts    TEXT NOT NULL
);
CREATE INDEX tasks_run ON tasks(run);
CREATE INDEX tasks_state ON tasks(state);

CREATE TABLE sessions (
  session_id    TEXT PRIMARY KEY,
  task          TEXT NOT NULL REFERENCES tasks(id),
  prefix_hash   TEXT NOT NULL,
  forked_from   TEXT REFERENCES sessions(session_id),
  jsonl_path    TEXT NOT NULL,
  created_ts    TEXT NOT NULL
);
CREATE INDEX sessions_task ON sessions(task);

CREATE TABLE capsules (
  id              TEXT PRIMARY KEY,
  task            TEXT NOT NULL REFERENCES tasks(id),
  kind            TEXT NOT NULL,
  from_role       TEXT NOT NULL REFERENCES roles(id),
  outcome         TEXT NOT NULL,
  rendered_tokens INTEGER NOT NULL,
  truncated       INTEGER NOT NULL,
  body_json       TEXT NOT NULL,
  created_ts      TEXT NOT NULL
);
CREATE INDEX capsules_task ON capsules(task);

CREATE TABLE decisions (
  id             TEXT PRIMARY KEY,
  title          TEXT NOT NULL,
  claim          TEXT NOT NULL,
  rationale      TEXT NOT NULL,
  origin_capsule TEXT REFERENCES capsules(id),
  supersedes     TEXT REFERENCES decisions(id),
  created_ts     TEXT NOT NULL
);

CREATE VIRTUAL TABLE decisions_fts USING fts5(
  title, claim, rationale, content='decisions', content_rowid='rowid'
);

CREATE TABLE artifacts (
  id      INTEGER PRIMARY KEY,
  capsule TEXT NOT NULL REFERENCES capsules(id),
  path    TEXT NOT NULL,
  symbol  TEXT,
  change  TEXT NOT NULL
);

CREATE TABLE symbols (
  fqname     TEXT PRIMARY KEY,
  path       TEXT NOT NULL,
  signature  TEXT,
  line_start INTEGER,
  line_end   INTEGER,
  blake3     TEXT NOT NULL
);

CREATE TABLE events (
  run        TEXT NOT NULL,
  seq        INTEGER NOT NULL,
  ts         TEXT NOT NULL,
  actor      TEXT NOT NULL,
  type       TEXT NOT NULL,
  scene_json TEXT NOT NULL,
  data_json  TEXT NOT NULL,
  PRIMARY KEY (run, seq)
);

CREATE TABLE token_ledger (
  id             INTEGER PRIMARY KEY,
  task           TEXT NOT NULL REFERENCES tasks(id),
  role           TEXT NOT NULL REFERENCES roles(id),
  prefix_hash    TEXT NOT NULL,
  estimate       INTEGER NOT NULL,
  input          INTEGER NOT NULL,
  output         INTEGER NOT NULL,
  cache_read     INTEGER NOT NULL,
  cache_creation INTEGER NOT NULL,
  cost_usd       REAL NOT NULL,
  model          TEXT NOT NULL,
  ts             TEXT NOT NULL
);
CREATE INDEX ledger_task ON token_ledger(task);
CREATE INDEX ledger_role_prefix ON token_ledger(role, prefix_hash);
CREATE UNIQUE INDEX ledger_one_live_estimate ON token_ledger(task) WHERE estimate = 1;

CREATE TABLE budgets (
  scope_kind   TEXT NOT NULL,
  scope_id     TEXT NOT NULL,
  limit_tokens INTEGER NOT NULL,
  spent_tokens INTEGER NOT NULL DEFAULT 0,
  usd_mirror   REAL NOT NULL DEFAULT 0,
  state        TEXT NOT NULL,
  PRIMARY KEY (scope_kind, scope_id)
);
"#;

const V2: &str = r#"
CREATE TRIGGER decisions_fts_insert AFTER INSERT ON decisions BEGIN
  INSERT INTO decisions_fts(rowid, title, claim, rationale)
  VALUES (new.rowid, new.title, new.claim, new.rationale);
END;

CREATE TRIGGER decisions_fts_delete AFTER DELETE ON decisions BEGIN
  INSERT INTO decisions_fts(decisions_fts, rowid, title, claim, rationale)
  VALUES ('delete', old.rowid, old.title, old.claim, old.rationale);
END;

CREATE TRIGGER decisions_fts_update AFTER UPDATE ON decisions BEGIN
  INSERT INTO decisions_fts(decisions_fts, rowid, title, claim, rationale)
  VALUES ('delete', old.rowid, old.title, old.claim, old.rationale);
  INSERT INTO decisions_fts(rowid, title, claim, rationale)
  VALUES (new.rowid, new.title, new.claim, new.rationale);
END;

INSERT INTO decisions_fts(rowid, title, claim, rationale)
  SELECT rowid, title, claim, rationale FROM decisions;
"#;

pub(crate) fn v1_for_test() -> &'static str {
    V1
}
