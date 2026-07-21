use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

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

    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

const V1: &str = r#"
CREATE TABLE files (
  path       TEXT PRIMARY KEY,
  lang       TEXT,
  blake3     TEXT NOT NULL,
  size       INTEGER NOT NULL,
  mtime      TEXT NOT NULL,
  is_binary  INTEGER NOT NULL
);

CREATE TABLE symbols (
  fqname     TEXT NOT NULL,
  path       TEXT NOT NULL,
  kind       TEXT NOT NULL,
  signature  TEXT,
  doc        TEXT,
  line_start INTEGER NOT NULL,
  line_end   INTEGER NOT NULL,
  PRIMARY KEY (fqname, path)
);
CREATE INDEX symbols_path ON symbols(path);

CREATE VIRTUAL TABLE symbols_fts USING fts5(fqname, signature, doc, path UNINDEXED);

CREATE TABLE refs (
  from_symbol TEXT NOT NULL,
  to_name     TEXT NOT NULL,
  path        TEXT NOT NULL,
  line        INTEGER NOT NULL
);
CREATE INDEX refs_to ON refs(to_name);
CREATE INDEX refs_from ON refs(from_symbol);
CREATE INDEX refs_path ON refs(path);
"#;
