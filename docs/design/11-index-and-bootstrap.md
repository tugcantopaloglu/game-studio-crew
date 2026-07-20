# 11: Index and Bootstrap

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
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
  fqname     TEXT PRIMARY KEY,   -- fully-qualified name
  path       TEXT NOT NULL REFERENCES files(path),
  kind       TEXT NOT NULL,      -- class | method | field | func | signal | ...
  signature  TEXT,
  doc        TEXT,               -- leading doc comment
  line_start INTEGER,
  line_end   INTEGER
);
CREATE VIRTUAL TABLE symbols_fts USING fts5(fqname, signature, doc, content='symbols', content_rowid='rowid');

CREATE TABLE refs (               -- symbol references; SYNTACTIC ONLY (13)
  from_symbol TEXT NOT NULL REFERENCES symbols(fqname),
  to_name     TEXT NOT NULL,      -- referenced name (may be unresolved)
  path        TEXT NOT NULL,
  line        INTEGER
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

## Tree-sitter extractors per language

Symbols and refs come from **tree-sitter** parsers, one grammar per language: C# (Unity), C++ (UE5 gameplay), GDScript (Godot), plus config/markup as needed. Extractors walk the parse tree to populate `symbols` and `refs`. **Refs are syntactic, not semantic**: tree-sitter sees names, not resolved types, so `refs.to_name` may be unresolved or ambiguous ([13](13-risks.md)); consumers treat refs as a strong hint, not a call graph, and the trust model's cross-file tiering ([10](10-standards-and-trust.md)) accounts for false edges.

## Asset extraction per engine

Beyond code, the index maps engine assets into `assets`/`scene_nodes`:

- **Unity:** parse YAML `.unity`/`.prefab` and `.meta` guids; build the scene node graph from the YAML hierarchy.
- **Godot:** parse `.tscn`/`.tres` text scenes into `scene_nodes` (native text format, cheap).
- **UE5:** `.umap`/`.uasset` are **binary** ([10](10-standards-and-trust.md), [13](13-risks.md)). Extraction relies on **asset-registry dumps** produced by the editor (a commandlet), not by parsing the binary directly. These dumps are expensive, so they are **debounced** (below) and their coverage is coarser than text scenes.

## Incremental freshness

The index is kept live with the **`notify`** filesystem watcher, gated on content hashes:

- A filesystem event triggers a **blake3 re-hash** of the changed file. If the hash is unchanged (touch, editor rewrite with identical bytes), **nothing is re-indexed**: the hash gate prevents redundant tree-sitter parses. If changed, only that file's symbols/refs/assets are re-extracted.
- **UE registry dumps are debounced.** Because dumping the asset registry is an expensive editor commandlet, `.umap`/`.uasset` changes don't trigger a dump per event; they set a dirty flag and a debounce timer (default a few seconds of quiescence) coalesces a burst of binary changes into one registry dump. This keeps a designer saving maps repeatedly from pinning the editor on back-to-back dumps.

Every index change emits `index_updated` ([05](05-event-protocol.md)) with the changed paths and symbol delta, so downstream consumers (context engine, trust model, floor) react without polling.
