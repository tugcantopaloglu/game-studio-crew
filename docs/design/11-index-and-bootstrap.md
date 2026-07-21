# 11: Index and Bootstrap

> **Status:** v0.5, 2026-07-21. The **code index and Godot asset extraction are built and wired**: `files`, `symbols`, `symbols_fts`, `refs`, `assets` and `scene_nodes` are all populated by `studio-index` from GDScript, C# and C++, `studiod index` builds them, `studiod studio` keeps them fresh around every command, and the `symbol_lookup` MCP tool serves slices out of them. **The `notify` watcher is measured and declined**, not deferred — see *Incremental freshness*. **Not built:** Unity and UE5 *asset* extraction, both unprobed engines ([13](13-risks.md) R11), so those sections below are specification, not description.
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
  asset      TEXT NOT NULL,
  node_path  TEXT NOT NULL,       -- hierarchy path within the scene, root is "."
  node_type  TEXT,
  script     TEXT,                -- project path of the script mounted on this node
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

Symbols and refs come from **tree-sitter** parsers, one grammar per language: C# (Unity), C++ (UE5 gameplay), GDScript (Godot), plus config/markup as needed. **All three are built.** Node type names for every grammar were read off real parse trees before the extractors were written, not guessed from grammar documentation.

### C++ needs a preprocessing pass, and finding that out was the point

Vanilla `tree-sitter-cpp` **cannot parse Unreal gameplay code**. Measured one construct at a time, plain C++ parses clean and then every UE reflection construct breaks it individually: `UCLASS()`, `GENERATED_BODY()`, `UPROPERTY()`, and the `GAME_API`-style export macro each produce a parse error on their own. Together they turn a class body into an `ERROR` node, and its members into stray labelled statements.

This matters more than it first appears. UE5 gameplay code is the *only* reason the C++ extractor exists, so an extractor built on the grammar's documented behaviour would have parsed the test suite's plain C++, passed, and produced silent garbage against every real Unreal file. That is R11's failure mode reproduced inside a component that has nothing to do with running an engine.

The fix is a preprocessing pass that **blanks reflection macros to spaces before parsing**: each macro and its balanced parenthesis group is overwritten with spaces, as is any `[A-Z0-9_]+_API` export macro. Newlines are preserved, so **the blanked source has the same length and the same line count as the original** and every reported line number still points at the real file. After blanking, the same fixture parses without error.

Two consequences worth naming. A C++ symbol appears **twice** — once for the declaration in the header, once for the definition in the `.cpp` — which the `(fqname, path)` key accommodates without collision, and which is the honest answer for a language that separates the two. And `calls` are filtered by path, so a header declaration does not inherit the call list of the body that implements it. Extractors walk the parse tree to populate `symbols` and `refs`. **Refs are syntactic, not semantic**: tree-sitter sees names, not resolved types, so `refs.to_name` may be unresolved or ambiguous ([13](13-risks.md)); consumers treat refs as a strong hint, not a call graph, and the trust model's cross-file tiering ([10](10-standards-and-trust.md)) accounts for false edges.

## Asset extraction per engine

Beyond code, the index maps engine assets into `assets`/`scene_nodes`:

- **Unity:** parse YAML `.unity`/`.prefab` and `.meta` guids; build the scene node graph from the YAML hierarchy.
- **Godot: built.** `.tscn`/`.tres` are parsed into `assets` and `scene_nodes` (native text format, cheap enough to hand-parse — no grammar needed). Node paths are built the way Godot addresses them: the root is `.`, and a child of `parent="Player"` becomes `Player/Sprite`, so a path in the index is a path you can paste into `get_node`. `script = ExtResource("1_p")` is resolved through the file's `ext_resource` table to a project path, which is what makes the script-to-scene direction queryable.

  **`scene_nodes.script` is an addition to the original schema.** Without it the index could describe a scene's shape but not answer the question a worker actually asks — *where is this script mounted?* A gameplay engineer editing `player.gd` needs to know it is the `CharacterBody2D` at `Player` in `scenes/main.tscn`, because that determines which node type's API is in scope. That link is the cheapest high-value edge in a Godot project and it lives on the node, so it is a column, not a table.
- **UE5:** `.umap`/`.uasset` are **binary** ([10](10-standards-and-trust.md), [13](13-risks.md)). Extraction relies on **asset-registry dumps** produced by the editor (a commandlet), not by parsing the binary directly. These dumps are expensive, so they are **debounced** (below) and their coverage is coarser than text scenes.

## Incremental freshness

**Built today:** refresh is a **scan** that walks the project, skipping VCS, editor and build directories — and `.studio/` itself, which the first real run caught the index feeding its own database into. Every file is hashed, and the hash gate below already applies: a second scan with no edits reparses nothing. Files that vanished between scans are dropped from the index.

The scan runs from two places. `studiod index [root]` does it on demand, and **`studiod studio` bootstraps the index at startup, then rescans both before and after every command**. Both hooks are load-bearing and they catch different writers:

- **After** a command, because workers write files. An index refreshed only at startup would answer the next task from a picture of code the studio itself has already changed.
- **Before** a command, because *humans* write files. The studio spends most of its life idle, and an edit made in an editor during that idle window would otherwise not reach the index until after the command that needed it had already finished. Refreshing only afterwards closed the worker-staleness window and left the human one open.

The hash gate makes running both affordable, and a refresh that moved nothing emits nothing, so the doubled call does not double the event stream.

**The `notify` watcher below is deliberately not built.** The argument for it was that a scan is O(files hashed) rather than O(edit), so it should not scale. Measured on a synthetic 4001-file Godot project (release build): a cold index costs **2.50s once**, and every subsequent refresh costs **0.24s** whether one file changed or none — roughly 60µs per file, so even a 40k-file project lands near 2.4s. Each command spawns `claude` workers that run for seconds to minutes, so the refresh is under one percent of the command it hangs off.

A watcher would not remove that cost so much as move it: `notify` needs a thread, debouncing, and tolerance for editors that write via temp-file-and-rename (which arrives as delete/create pairs), and because it can drop events under load a periodic reconciling scan has to stay anyway. It is a second mechanism layered on the one that already works, bought with a measured sub-one-percent saving. The number is what makes this a decision rather than a deferral; if a project ever makes the refresh visible, it is written down here at what scale to revisit.

The index is kept live with the **`notify`** filesystem watcher, gated on content hashes:

- A filesystem event triggers a **blake3 re-hash** of the changed file. If the hash is unchanged (touch, editor rewrite with identical bytes), **nothing is re-indexed**: the hash gate prevents redundant tree-sitter parses. If changed, only that file's symbols/refs/assets are re-extracted.
- **UE registry dumps are debounced.** Because dumping the asset registry is an expensive editor commandlet, `.umap`/`.uasset` changes don't trigger a dump per event; they set a dirty flag and a debounce timer (default a few seconds of quiescence) coalesces a burst of binary changes into one registry dump. This keeps a designer saving maps repeatedly from pinning the editor on back-to-back dumps.

Every index change emits `index_updated` ([05](05-event-protocol.md)) with the changed paths and symbol delta, so downstream consumers (context engine, trust model, floor) react without polling. **This is emitted today** on every refresh that actually moved something, carrying `paths_changed`, `symbols_delta` and a capped sample of the paths; the floor turns the running total into a `symbols` figure beside cache hit rate. A refresh that changed nothing emits nothing, so the event stream stays a record of real change rather than a heartbeat.
