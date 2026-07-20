# 03: State Store

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **This document is the single source of truth for the state-store SQLite schema.** It is a **distinct database** from the code/asset **index** ([11](11-index-and-bootstrap.md)), different file, different connection, never conflated. [01](01-orchestrator-core.md) and [06](06-budget-governance.md) read/write these tables; they do not redefine them.

## Shape

One SQLite file, `studio-state.db`, in **WAL mode**, behind a **single writer actor** over `rusqlite`. All mutations go through one Tokio task that owns the write connection and serializes writes; readers use a pool of read-only connections (WAL allows concurrent readers during a write). This gives us serializable writes without a server, and the event bus ([05](05-event-protocol.md)) is a natural consumer of the same writer actor's ordering (it assigns `seq`).

Why single-writer over a connection pool with `BEGIN IMMEDIATE` everywhere: the ledger and event `seq` need a total order, and one writer gives it for free without retry-on-busy loops.

## Tables

DDL is illustrative but complete enough to build against.

```sql
-- Roles: a cache/mirror of the registry in 04; the registry file is authoritative,
-- this table exists so other tables can FK to a role and so cache_hit_ratio can group by it.
CREATE TABLE roles (
  id            TEXT PRIMARY KEY,        -- e.g. 'gameplay_engineer' (see 04)
  tier          INTEGER NOT NULL,        -- 1|2|3
  department    TEXT NOT NULL,
  model         TEXT NOT NULL,           -- 'fable' | 'opus' | 'haiku' (summarizer only)
  effort        TEXT NOT NULL,
  escalates_to  TEXT REFERENCES roles(id)
);

CREATE TABLE tasks (
  id            TEXT PRIMARY KEY,        -- 'task_01J...'
  run           TEXT NOT NULL,
  role          TEXT NOT NULL REFERENCES roles(id),
  parent_task   TEXT REFERENCES tasks(id),
  workflow_node TEXT,                    -- 09 node id, null for ad-hoc
  state         TEXT NOT NULL,           -- worker lifecycle state (01)
  outcome       TEXT,                    -- outcome enum (01), null until terminal
  created_ts    TEXT NOT NULL,
  updated_ts    TEXT NOT NULL
);

CREATE TABLE sessions (                  -- claude CLI sessions (JSONL under ~/.claude/projects)
  session_id    TEXT PRIMARY KEY,
  task          TEXT NOT NULL REFERENCES tasks(id),
  prefix_hash   TEXT NOT NULL,           -- blake3 of the frozen L0..L2 prefix (02)
  forked_from   TEXT REFERENCES sessions(session_id),  -- horizontal consult (04)
  jsonl_path    TEXT NOT NULL,           -- for crash recovery (01)
  created_ts    TEXT NOT NULL
);

CREATE TABLE capsules (                  -- schema + truncation defined in 02
  id            TEXT PRIMARY KEY,
  task          TEXT NOT NULL REFERENCES tasks(id),
  kind          TEXT NOT NULL,           -- task_return|consult_answer|decision|escalation|status
  from_role     TEXT NOT NULL REFERENCES roles(id),
  outcome       TEXT NOT NULL,
  rendered_tokens INTEGER NOT NULL,
  truncated     INTEGER NOT NULL,        -- bool
  body_json     TEXT NOT NULL,           -- the validated capsule
  created_ts    TEXT NOT NULL
);

CREATE TABLE decisions (                 -- ADR store (02)
  id            TEXT PRIMARY KEY,
  title         TEXT NOT NULL,
  claim         TEXT NOT NULL,
  rationale     TEXT NOT NULL,
  origin_capsule TEXT REFERENCES capsules(id),
  supersedes    TEXT REFERENCES decisions(id),
  created_ts    TEXT NOT NULL
);
CREATE VIRTUAL TABLE decisions_fts USING fts5(title, claim, rationale, content='decisions', content_rowid='rowid');

CREATE TABLE artifacts (                 -- files/symbols capsules touched, by reference
  id            INTEGER PRIMARY KEY,
  capsule       TEXT NOT NULL REFERENCES capsules(id),
  path          TEXT NOT NULL,
  symbol        TEXT,
  change        TEXT NOT NULL            -- added|modified|removed
);

CREATE TABLE symbols (                   -- symbol slice cache fed from the index (11) for L3 assembly
  fqname        TEXT PRIMARY KEY,
  path          TEXT NOT NULL,
  signature     TEXT,
  line_start    INTEGER,
  line_end      INTEGER,
  blake3        TEXT NOT NULL            -- freshness gate; matches index row or is restaled
);

CREATE TABLE events (                    -- 05 envelope, persisted for resume/replay
  seq           INTEGER PRIMARY KEY,     -- monotonic, gap-free per run via the writer actor
  run           TEXT NOT NULL,
  ts            TEXT NOT NULL,
  actor         TEXT NOT NULL,
  type          TEXT NOT NULL,
  scene_json    TEXT NOT NULL,
  data_json     TEXT NOT NULL
);
CREATE INDEX events_run_seq ON events(run, seq);

CREATE TABLE token_ledger (              -- the ground truth 02's estimates get replaced by
  id            INTEGER PRIMARY KEY,
  task          TEXT NOT NULL REFERENCES tasks(id),
  role          TEXT NOT NULL REFERENCES roles(id),
  prefix_hash   TEXT NOT NULL,
  estimate      INTEGER NOT NULL,        -- bool: interim estimate vs final result
  input         INTEGER NOT NULL,
  output        INTEGER NOT NULL,
  cache_read    INTEGER NOT NULL,
  cache_creation INTEGER NOT NULL,
  cost_usd      REAL NOT NULL,
  model         TEXT NOT NULL,
  ts            TEXT NOT NULL
);
CREATE INDEX ledger_task ON token_ledger(task);
CREATE INDEX ledger_role_prefix ON token_ledger(role, prefix_hash);

CREATE TABLE budgets (                   -- task & sprint budgets (06)
  scope_kind    TEXT NOT NULL,           -- 'task' | 'sprint'
  scope_id      TEXT NOT NULL,
  limit_tokens  INTEGER NOT NULL,
  spent_tokens  INTEGER NOT NULL DEFAULT 0,
  usd_mirror    REAL NOT NULL DEFAULT 0, -- derived, display only
  state         TEXT NOT NULL,           -- ok|warned|degrading|stopped (06)
  PRIMARY KEY (scope_kind, scope_id)
);
```

## Key queries

### Realtime spend (drives the budget enforcer and the floor's spend readout)

```sql
-- Final rows are authoritative; estimate rows fill the gap for in-flight workers (06).
SELECT COALESCE(SUM(CASE WHEN estimate=0 THEN input+output ELSE 0 END), 0)
     + COALESCE(SUM(CASE WHEN estimate=1 THEN input+output ELSE 0 END), 0) AS tokens,
       COALESCE(SUM(cost_usd), 0) AS usd
FROM token_ledger
WHERE task IN (SELECT id FROM tasks WHERE run = ?1);
```

The enforcer prefers the `estimate=0` row for any task that has one and falls back to the latest `estimate=1` row otherwise (the writer actor upserts estimates keyed on `task`, so there is one live estimate per in-flight worker that a final row supersedes).

### Per-role `cache_hit_ratio` (the broken-prefix detector)

```sql
-- A healthy frozen prefix reads from cache on warm spawns. A ratio near zero for a
-- role whose charter didn't change means a byte-stability regression (02). The
-- single most useful health metric in the system.
SELECT role, prefix_hash,
       SUM(cache_read) AS reads,
       SUM(cache_creation) AS writes,
       CAST(SUM(cache_read) AS REAL) / NULLIF(SUM(cache_read)+SUM(cache_creation), 0) AS cache_hit_ratio
FROM token_ledger
WHERE estimate = 0 AND ts >= ?1
GROUP BY role, prefix_hash;
```

A role showing multiple distinct `prefix_hash` values in a window where its charter was unchanged is the smoking gun for a silent invalidator; the daemon alarms on it and the floor surfaces it ([12](12-visual-workspace.md)).

## Crash recovery hook

`sessions.jsonl_path` + `tasks.state` are what [01](01-orchestrator-core.md) reads to resume: on restart, any task in a non-terminal state has its CLI session resumed (`--resume <session_id>`) from the persisted JSONL rather than restarted from scratch.

## Migrations

`schema_version` in a `meta` table; forward-only migrations run at startup inside the writer actor before it accepts writes. WAL checkpointing on a size threshold; the DB is disposable per-project but survives container restarts only if committed to the project's data dir. An ephemeral-container caveat noted in [00](00-overview.md).
