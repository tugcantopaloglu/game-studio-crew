# 11: Index and Bootstrap

> **Status:** v0.2, 2026-07-21. The **code index is built and wired**: `files`, `symbols`, `symbols_fts` and `refs` are populated by `studio-index` from GDScript and C# sources, `studiod index` builds them, and the `symbol_lookup` MCP tool serves slices out of them. **Asset extraction is not built**: `assets` and `scene_nodes` remain design-only, as does the `notify` watcher, the `index_updated` event, the C++ extractor and the UE registry dump. Those sections below are specification, not description.
> **This document is the single source of truth for the index SQLite schema.** It is a **distinct database** from the runtime **state store** ([03](03-state-store.md)), different file (`studio-index.db`), different lifecycle, never conflated. The context engine ([02](02-context-engine.md)) reads the index to build symbol slices; the standards layer ([10](10-standards-and-trust.md)) reads diffs and refs from it.

## What bootstrap does

On first run in a project, the daemon (1) detects the engine(s), (2) resolves the toolchain, (3) installs engine-side helpers, (4) builds the code/asset index, then (5) keeps the index fresh incrementally. This is what makes L3 briefs ([02](02-context-engine.md)) small. The index is queried for exactly the symbols a task names, so file bodies are never inlined by default.

## Engine detection heuristics with precedence

Detection uses the `[detect]` block of each engine profile ([07](07-engine-layer.md)): a marker set plus a `precedence` integer.

- **Markers** are must-exist paths: Unity → `ProjectSettings/ProjectVersion.txt`; UE5 → a `*.uproject` file; Godot → `project.godot`.
- **Precedence** breaks ties when a repo trips more than one marker set (e.g. a tools subdirectory that looks Godot-ish inside a Unity project). Higher precedence wins; the loser is recorded as a secondary engine only if it has its own project root.
- A monorepo with genuinely separate project roots yields multiple active engines, each with its own profile binding. The studio operates all three from the same 13 roles ([04](04-agent-graph.md)).

Detection result and confidence are logged; ambiguous detection asks rather than guesses.

## Tooling resolution

Per the profile `[tooling]` block ([07](07-engine-layer.md)): resolve the engine binary via, in order, an explicit env var (`UNITY_EDITOR`, `UE_ROOT`, `GODOT_BIN`), then a known launcher (Unity Hub), then `PATH`. The resolved absolute path is cached; a version mismatch against the profile's `min_editor_version` is a warning surfaced at bootstrap, not a silent failure.

## Engine-side helper installation

Some commands need an in-project helper the daemon drives:

- **Unity:** a `Studio.CI` static class ([07](07-engine-layer.md)) copied into an editor-only asmdef, exposing `Compile`/`Reimport`/`BuildPlayer` for `-executeMethod`.
- **Godot:** the **GUT** addon under `addons/gut/` for `test_fast`/`test_full`, plus **`addons/studio/studio_ci.gd`** for `compile`. The latter exists because Godot's own `--check-only` only reaches scripts referenced by the main scene and always exits 0 ([07](07-engine-layer.md)); the helper walks all of `res://` and returns a real exit code. It is written on install and rewritten whenever its bytes differ from the daemon's copy, so a tampered helper is restored rather than trusted.
- **UE5:** no code helper: the automation runner and UAT are built in; bootstrap only verifies they're present.

Helpers are installed idempotently and version-pinned; they live in the project so verification is reproducible.

## Index schema

```sql
CREATE TABLE files (
  path       TEXT PRIMARY KEY,
  lang       TEXT,               -- csharp | cpp | gdscript | ...
  blake3     TEXT NOT NULL,      -- content hash, the freshness gate
  size       INTEGER,
  mtime      TEXT,
  is_binary  INTEGER NOT NULL    -- 1 for .umap/.uasset/etc (10, 13)
);

CREATE TABLE symbols (
  fqname     TEXT NOT NULL,      -- fully-qualified name
  path       TEXT NOT NULL,
  kind       TEXT NOT NULL,      -- class | method | field | func | signal | ...
  signature  TEXT,
  doc        TEXT,               -- leading doc comment
  line_start INTEGER NOT NULL,
  line_end   INTEGER NOT NULL,
  PRIMARY KEY (fqname, path)
);
CREATE VIRTUAL TABLE symbols_fts USING fts5(fqname, signature, doc, path UNINDEXED);

CREATE TABLE refs (               -- symbol references; SYNTACTIC ONLY (13)
  from_symbol TEXT NOT NULL,
  to_name     TEXT NOT NULL,      -- referenced name (may be unresolved)
  path        TEXT NOT NULL,
  line        INTEGER NOT NULL
);
CREATE INDEX refs_to ON refs(to_name);

CREATE TABLE assets (             -- engine assets, extracted per engine
  path       TEXT PRIMARY KEY REFERENCES files(path),
  asset_type TEXT NOT NULL,       -- prefab | scene | material | blueprint | umap | ...
  guid       TEXT,                -- Unity .meta guid / UE asset path
  blake3     TEXT NOT NULL
);

CREATE TABLE scene_nodes (        -- scene/prefab/map graph, extracted per engine
  id         INTEGER PRIMARY KEY,
  asset      TEXT NOT NULL REFERENCES assets(path),
  node_path  TEXT NOT NULL,       -- hierarchy path within the scene
  node_type  TEXT,
  parent     INTEGER REFERENCES scene_nodes(id)
);
```

The symbol slice the context engine pulls ([02](02-context-engine.md)) is `symbols` + a one-hop `refs` neighborhood; the diff/blast-radius the trust model uses ([10](10-standards-and-trust.md)) is computed from `files.blake3` deltas plus `refs`.

Three details of the schema above changed when it met the implementation, and the reasons generalise:

- **`symbols` is keyed on `(fqname, path)`, not `fqname` alone.** A bare `fqname` primary key silently drops one of two same-named symbols in different files, which is common the moment a project has `a/util.gd` and `b/util.gd`. Over-keying loses nothing; under-keying loses symbols with no error.
- **`refs.from_symbol` carries no foreign key.** Refs are syntactic ([13](13-risks.md) R8), so `to_name` is routinely unresolved by design; making the *source* side a strict FK while the target side is deliberately loose bought consistency the data model does not actually have.
- **`symbols_fts` is a standalone FTS5 table, not an external-content one.** External content requires issuing matched `delete` commands with the pre-edit values on every reindex; a standalone table is cleared by `DELETE ... WHERE path = ?` along with the file's other rows. The cost is a duplicated copy of `fqname`/`signature`/`doc`; the gain is that stale search hits cannot outlive a reindex. A test pins exactly that.

**Lookup resolves names before it searches text.** `symbol_lookup` tries the exact `fqname`, then a `.name` suffix match, and only falls back to full-text search when both miss *and* the query carries no dot. A dotted query is a name, not a search: answering `Player.take_damage` with `Player.heal` because they share a prefix is worse than answering nothing.

## Tree-sitter extractors per language

Symbols and refs come from **tree-sitter** parsers, one grammar per language: C# (Unity), C++ (UE5 gameplay), GDScript (Godot), plus config/markup as needed. **GDScript and C# are built; C++ is not** — a `.cpp` or `.h` file is tracked in `files` so its hash and blast radius are known, but it yields no symbols yet. Node type names for both built grammars were read off real parse trees before the extractors were written, not guessed from grammar documentation. Extractors walk the parse tree to populate `symbols` and `refs`. **Refs are syntactic, not semantic**: tree-sitter sees names, not resolved types, so `refs.to_name` may be unresolved or ambiguous ([13](13-risks.md)); consumers treat refs as a strong hint, not a call graph, and the trust model's cross-file tiering ([10](10-standards-and-trust.md)) accounts for false edges.

## Asset extraction per engine

Beyond code, the index maps engine assets into `assets`/`scene_nodes`:

- **Unity:** parse YAML `.unity`/`.prefab` and `.meta` guids; build the scene node graph from the YAML hierarchy.
- **Godot:** parse `.tscn`/`.tres` text scenes into `scene_nodes` (native text format, cheap).
- **UE5:** `.umap`/`.uasset` are **binary** ([10](10-standards-and-trust.md), [13](13-risks.md)). Extraction relies on **asset-registry dumps** produced by the editor (a commandlet), not by parsing the binary directly. These dumps are expensive, so they are **debounced** (below) and their coverage is coarser than text scenes.

## Incremental freshness

**Built today:** refresh is a **scan** that walks the project, skipping VCS, editor and build directories — and `.studio/` itself, which the first real run caught the index feeding its own database into. Every file is hashed, and the hash gate below already applies: a second scan with no edits reparses nothing. Files that vanished between scans are dropped from the index.

The scan runs from two places. `studiod index [root]` does it on demand, and **`studiod studio` bootstraps the index before it accepts a single command, then rescans after each one completes**. That second hook is what keeps the index honest in a live studio: workers write files, so an index built only at startup would answer the next task from a stale picture of code the studio itself changed. The hash gate is what makes rescanning after every command affordable — a command that touched nothing reparses nothing and stays silent.

**Not built:** the watcher that makes this incremental without a scan. A rescan is O(files hashed), not O(files parsed), which is cheap enough for a game project but is still work proportional to repository size rather than to the edit.

The index is kept live with the **`notify`** filesystem watcher, gated on content hashes:

- A filesystem event triggers a **blake3 re-hash** of the changed file. If the hash is unchanged (touch, editor rewrite with identical bytes), **nothing is re-indexed**: the hash gate prevents redundant tree-sitter parses. If changed, only that file's symbols/refs/assets are re-extracted.
- **UE registry dumps are debounced.** Because dumping the asset registry is an expensive editor commandlet, `.umap`/`.uasset` changes don't trigger a dump per event; they set a dirty flag and a debounce timer (default a few seconds of quiescence) coalesces a burst of binary changes into one registry dump. This keeps a designer saving maps repeatedly from pinning the editor on back-to-back dumps.

Every index change emits `index_updated` ([05](05-event-protocol.md)) with the changed paths and symbol delta, so downstream consumers (context engine, trust model, floor) react without polling. **This is emitted today** on every refresh that actually moved something, carrying `paths_changed`, `symbols_delta` and a capped sample of the paths; the floor turns the running total into a `symbols` figure beside cache hit rate. A refresh that changed nothing emits nothing, so the event stream stays a record of real change rather than a heartbeat.
