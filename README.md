<div align="center">

<img src="images/logo.png" alt="Game Studio Crew" width="180">

# Game Studio Crew

**A game studio that runs itself.**

Thirteen AI specialists, a producer, a director — working a real project in a real engine,
on a studio floor you can watch in 3D while it happens.

Rust daemon · Unity · Unreal Engine 5 · Godot 4 · No API keys

**Windows · macOS · Linux**

</div>

---

## What this is

You describe a game. A director decomposes it into tasks, hands each one to the specialist who should do it, and runs them in parallel. Engineers write code, artists draw and rig the assets, QA verifies against the real engine, and the whole thing commits itself to git as it goes.

You watch all of it happen on a top-down 3D studio floor in your browser — desks lighting up, characters walking to meetings, a timeline you can scrub.

There are **no API keys anywhere in this**. Everything runs through the coding CLI subscription you already pay for.

---

## Meet the crew

Thirteen roles across six departments. An engine is a *prompt layer*, not a separate cast — the same crew works Unity, Unreal and Godot.

### Leadership

<table>
<tr>
<td align="center" width="25%"><img src="agent-images/studio_director.png" width="130"><br><b>Studio Director</b><br><sub>Tier 1 · Fable</sub><br><sub>Owns the plan. Answers to you.</sub></td>
<td align="center" width="25%"><img src="agent-images/producer.png" width="130"><br><b>Producer</b><br><sub>Tier 2 · Opus</sub><br><sub>Scope, sprints, release gates.</sub></td>
<td align="center" width="25%"><img src="agent-images/senior_engineer.png" width="130"><br><b>Systems &amp; Tools Engineer</b><br><sub>Tier 2 · Opus</sub><br><sub>Architecture and the build.</sub></td>
<td align="center" width="25%"><img src="agent-images/game_designer.png" width="130"><br><b>Game Designer</b><br><sub>Tier 2 · Opus</sub><br><sub>Mechanics and the feel of them.</sub></td>
</tr>
</table>

### Engineering & QA

<table>
<tr>
<td align="center" width="25%"><img src="agent-images/gameplay_engineer.png" width="130"><br><b>Gameplay Engineer</b><br><sub>Tier 3 · Opus</sub><br><sub>Systems that ship in the game.</sub></td>
<td align="center" width="25%"><img src="agent-images/infrastructure_engineer.png" width="130"><br><b>Build &amp; Infra Engineer</b><br><sub>Tier 3 · Opus</sub><br><sub>Pipelines, CI, packaging.</sub></td>
<td align="center" width="25%"><img src="agent-images/qa_engineer.png" width="130"><br><b>QA Engineer</b><br><sub>Tier 3 · Opus</sub><br><sub>Reproduces, verifies, refuses.</sub></td>
<td align="center" width="25%"><img src="agent-images/technical_artist.png" width="130"><br><b>Technical Artist</b><br><sub>Tier 3 · Opus</sub><br><sub>Rigs, shaders, the art pipeline.</sub></td>
</tr>
</table>

### Design, Art & Audio

<table>
<tr>
<td align="center" width="25%"><img src="agent-images/level_designer.png" width="130"><br><b>Level Designer</b><br><sub>Tier 3 · Opus</sub><br><sub>Spaces, pacing, encounters.</sub></td>
<td align="center" width="25%"><img src="agent-images/narrative_designer.png" width="130"><br><b>Narrative Designer</b><br><sub>Tier 3 · Opus</sub><br><sub>Story, dialogue, tone.</sub></td>
<td align="center" width="25%"><img src="agent-images/ui-ux_designer.png" width="130"><br><b>UX/UI Designer</b><br><sub>Tier 3 · Opus</sub><br><sub>Menus, HUD, readability.</sub></td>
<td align="center" width="25%"><img src="agent-images/game_artist.png" width="130"><br><b>Artist</b><br><sub>Tier 3 · Opus</sub><br><sub>Sprites, textures, characters.</sub></td>
</tr>
<tr>
<td align="center" width="25%"><img src="agent-images/audio_designer.png" width="130"><br><b>Audio Designer</b><br><sub>Tier 3 · Opus</sub><br><sub>Music, SFX, mix.</sub></td>
<td colspan="3"></td>
</tr>
</table>

Every role has a **frozen charter** — a byte-identical system prompt that never changes between invocations, so the model provider's prompt cache pays for it once. Each role also carries only the tools its job needs: coordination roles get none at all, designers and engineers get read and write, art and QA additionally get a shell.

---

## Quick start

Runs on **Windows, macOS and Linux**. Grab a build from [Releases](../../releases), or build from source.

### Windows

Unzip and run `game-studio.exe`. For a Start Menu entry and a proper uninstaller:

```powershell
powershell -ExecutionPolicy Bypass -File installer\build.ps1
```

That compiles the daemon, the native shell and a per-user installer in one command. The installer needs **no admin rights** and touches neither `PATH` nor any service. On uninstall it asks before removing the studio's own data — your project folders live where you put them and are never touched.

### macOS

```bash
tar xzf game-studio-crew-1.0.0-macos-aarch64.tar.gz
mv "Game Studio Crew.app" /Applications/
```

The build is unsigned, so the first launch needs right-click → **Open**.

### Linux

```bash
tar xzf game-studio-crew-1.0.0-linux-x86_64.tar.gz
cd game-studio-crew-1.0.0-linux-x86_64
./install.sh          # installs to ~/.local by default, no root
```

The desktop shell needs `libwebkit2gtk-4.1`; the daemon alone has no such dependency.

### From source

```bash
cargo build --release
./target/release/studiod doctor     # what's installed, and what can actually run the crew
./target/release/studiod studio     # serve the floor at http://127.0.0.1:7878

cd desktop && cargo build --release # the native window, optional
```

On Linux, install the webview headers first: `libwebkit2gtk-4.1-dev libgtk-3-dev`.

### The desktop window

The window is a thin native frame around the same floor the daemon serves on `127.0.0.1:7878`. It starts the daemon for you, waits for the port rather than guessing, and takes the daemon and every worker down with it when you close it — on every platform. If a daemon is already running it attaches to that one instead of starting a second.

### What you need

`studiod doctor` deliberately tells you two different things: **which CLIs you have**, and **which ones can actually spawn a worker**.

| | Status |
|---|---|
| **A coding CLI** | `claude`, `codex`, `gemini`, `copilot` and `kimi` are all detected. Any one is enough to install — but only a CLI that accepts a frozen charter can run the crew, which today means **`claude`**. |
| **git** | Optional. Reported present or absent; its absence is never an error. |
| **An engine** | Optional. Godot, Unity or Unreal, whichever you're building in. |
| **Art pipeline** | Optional. `node`, a `python` that can import `pillow`, and the `codex` imagegen skill. |

An install that finds only Gemini succeeds — and then says plainly that nothing installed can start a worker yet, and exactly what to install, instead of showing a tick you'd learn the truth about at your first task. `studiod doctor --fix` runs the installs it can after showing you the list and asking. The one step it can't do for you is `codex login`, and it says so.

---

## The studio floor

The floor is a top-down 3D office rendered in the browser. It is not a log viewer — it is the studio.

- **Desks light up** when a worker spawns, and settle when it exits.
- **Characters walk** to the meeting room when a room is convened, and back to their desks after.
- **Shape encodes identity** — outfit, hair and the prop in hand are per-role, so you can tell the QA engineer from the audio designer at floor distance.
- **A second storey** holds management, reached by a working lift.
- **Click anyone** to inspect what they're doing, what they were briefed with, and what they cost.
- **A timeline scrubber** replays the run; a minimap keeps you oriented.
- **A workflow track** shows the DAG executing, node by node.
- Live **token and cost readouts**, cache hit rates, and a frame-time counter where you'd actually look for it.

Everything on the floor is driven by a real event feed over WebSocket — the same events that go into the state store, replayed on reconnect from the last sequence number you saw.

---

## What you can do

### Projects

Work is scoped to a project: a directory you name, at a path you choose, with its own git repo. Create one from the floor — name, absolute path, engine, and whether to `git init` — and every task, workflow and build afterwards runs with that directory as the working directory.

**Nothing runs without a project selected.** There is no fallback to the daemon's own directory.

You can also **adopt an existing game**: point the studio at a folder, and it surveys the tree, detects the engine, indexes the code and writes a summary the crew plans against, so tasks extend what's there instead of rebuilding it.

### Tasks

Send a single role a single brief. It spawns, works in the project, and commits.

### Builds

Describe what you want. The director returns a plan — a DAG of tasks with roles, briefs and dependencies — which you can **edit before anything runs**: change a brief, drop a step, reorder. Then the crew executes it, up to four workers in parallel.

Turn on **guided mode** and the run holds after every step for your approval, with an option to send a step back with a note ("make the pipes green") up to three times.

### Meetings

Convene a room and the daemon runs a real deliberation, not a transcript. Each participant after the first is handed the previous positions **verbatim** and asked to answer them. The chair — the nearest common ancestor in the escalation tree — receives the whole room and is held to a schema, so it returns a rule the studio will follow, the reason it beat the alternative, and the positions it overrules.

The ruling lands in two places: the decision store, so it can be handed to a worker weeks later, and `docs/decisions/` in the project repo as an ADR recording the claim, the reasoning, the dissent and the whole room. A chair that returns nothing usable adjourns the meeting — the studio never records a decision nobody made.

### Workflows

Four DAG workflows ship built in, defined in TOML with nodes, edges and gates:

| Workflow | What it does |
|---|---|
| `feature` | design → implement → verify → polish |
| `bugfix` | triage → reproduce → fix → regression-test |
| `release` | freeze → verify → export → package |
| `sprint_planning` | scope → break down → estimate → schedule |

### Verification and repair

When a gate fires, the daemon runs the engine's own build and test commands, parses the structured report, and hands any failures back to a repair worker with the exact list — up to three rounds.

The verify contract is **inconclusive-first**: a missing report, an empty suite, a truncated report, a crash exit code or a silent CI helper are all *inconclusive*, never a pass. Absence of evidence is never treated as evidence. Infrastructure problems (a licence server, a missing GPU) are routed away from the crew rather than handed to an engineer to "fix".

The gate also **re-verifies its own tooling before every run** — the studio's CI helper lives inside the project, so if anything rewrote it, the gate restores it from the shipped copy and fails by name rather than trusting a check the work under test could have written.

### Art the crew draws itself

The crew can make its own assets: it asks `codex` for a sprite or texture, keys the flat background out of it, and turns a character into a **rigged, animated model the engine loads** — including retargeting Mixamo clips onto the generated skeleton. A character nothing can pose is failed, not shipped.

```bash
studiod asset character --name hero --describe "a small knight in dented brass armour"
studiod asset sprite    --name coin --describe "a spinning gold coin"
studiod asset rig       --slug hero
studiod asset animate   --slug hero --fbx anims/run.fbx
```

This path is optional in the honest sense: with any of it missing, every other path behaves exactly as before and the crew builds art by hand.

### Version control

The daemon commits for you. After each worker completes it stages the project and writes a commit subject of `<role>: <first line of the brief>`, so history reads as ordinary studio work and you can diff or revert any single worker's output.

**Committing is daemon work, not worker work.** No worker ever runs git or receives git tools, and no commit message is model-generated — so version control costs **zero tokens**. From the floor you get a commit tree, per-node revert, rollback with a preview of exactly what's about to be destroyed, and push.

### Budget governance

You set a ceiling in tokens, in dollars, or both. The daemon projects every spawn before it happens and admits, degrades or refuses it.

As pressure rises the crew degrades gracefully rather than stopping dead — leaning on reasoning effort first, routing summarization to a cheaper model, trimming context — with the rung following actual budget pressure rather than the number of spawns. You can also ask to be prompted above a spend threshold, and stop a run at any point from the floor.

Every worker's real usage is recorded per turn — including workers that were killed or timed out, which are exactly the runs that cost the most.

### Code index

A tree-sitter index parses GDScript, C# and C++ into a separate database, gated on content hashes so an unchanged file is never re-parsed. Workers ask it for **symbols, not files**.

A worker learns that `Enemy.attack` calls `Player.take_damage`, and that it runs on the `CharacterBody2D` at `Player` in `scenes/main.tscn` — without any of those files entering its context. Measured against a worker with `Read`, `Grep`, `Glob` and no index on the same question: **2.3–3.4× fewer input tokens**, same correct answer.

### Settings

Per-tier and per-role model and effort overrides, engine binary paths, budgets, parallelism, and music — all from the floor, all live.

---

## How it works

The daemon owns everything. Workers are cheap, disposable, and told only what they need for one task.

```mermaid
graph LR
  subgraph Daemon["Rust daemon"]
    ORCH[orchestrator<br/>supervisor + budget]
    CTX[context engine<br/>frozen charters]
    STATE[(state store<br/>SQLite/WAL)]
    IDX[(code index<br/>SQLite/WAL)]
    EVT[event bus]
    MCP[MCP server]
  end
  subgraph Workers["CLI workers (stateless)"]
    W1[worker]
    W2[worker]
  end
  ENG[engine drivers<br/>Unity / UE5 / Godot]
  UI[studio floor]

  CTX --> ORCH
  STATE <--> ORCH
  IDX --> CTX
  ORCH --> W1 & W2
  W1 & W2 -. tool calls .-> MCP
  MCP --> ORCH
  ORCH --> ENG
  ENG -. structured failures .-> ORCH
  ORCH --> EVT --> UI
```

A worker reads a frozen system prompt, receives one volatile task brief, does the work, and exits. It never talks to another worker and never holds durable state. The daemon reduces its output into events, ledger rows and state transitions.

### The crates

| Crate | Owns |
|---|---|
| `studio-core` | worker supervision, process trees, watchdogs, stream parsing, git |
| `studio-context` | layered prompts, charter freezing and hashing, capsules, summarization |
| `studio-store` | runtime state, event log, token ledger — single-writer SQLite |
| `studio-index` | engine detection, tree-sitter extractors, the code/asset index |
| `studio-agents` | the 13-role registry, delegation and escalation |
| `studio-events` | event envelope and enum, coalescing, resume |
| `studio-budget` | budgets, enforcement points, the degradation ladder |
| `studio-engine` | engine profiles, charter fragments, helper bootstrap |
| `studio-verify` | the `verify()` contract, report parsers, the repair loop |
| `studio-workflow` | TOML DAG workflows, the parallel executor, gates |
| `studio-standards` | rule modes and the R0–R4 trust model |
| `studio-mcp` | the studio's MCP tools over stdio |
| `studio-settings` | settings, providers, model and effort resolution |
| `studio-server` | HTTP + WebSocket server and the studio floor |
| `studiod` | the binary that ties it together |

Two **separate** SQLite databases: runtime state and the code index. They are never the same file and never share a connection.

---

## Why it's cheap

> **Feed the model minimum viable context, and never pay twice for the same tokens.**

- **The tool list is the primary lever.** Built-in tool schemas, not project files, are the bulk of a default invocation: the same call costs **22,572** tokens with the default tool set and **184** with an empty one.
- **Charters are byte-frozen and content-hashed**, so prompt caching (1-hour TTL, keyed on exact prompt bytes plus tool set plus model) pays for them once, and every same-role worker in the window reads from cache.
- **13 roles, not 50.** Fewer distinct prefixes means fewer cold starts — and a cold start costs a **2.0× write premium**.
- **Cold starts are staggered.** When a wave holds several workers sharing a prefix, one goes first and warms it; the rest follow warm instead of each paying the write premium.
- **The daemon summarizes**, at zero token cost, so briefs stay small.
- **Symbols, not files**, from the index.

Measured: a warm prefix costs **$0.0025 against $0.0374 cold — a 14.8× reduction** across separate subprocesses, against **$0.2258 undefended**. These are probe measurements, not estimates. See [`probes/`](probes/README.md).

---

## Engines

| Engine | Status |
|---|---|
| **Godot 4** | Proven end to end — build, verify, repair, export |
| **Unity** | Profile written, commands bound, **not yet run against a real editor** |
| **Unreal Engine 5** | Profile written, commands bound, **not yet run against a real editor** |
| **Web / Python** | Supported for prototypes and probes |

An engine is a *profile plus a driver*, not a fork of the crew: each ships its build, test, import and export command lines plus prose fragments injected into charters. The same 13 roles operate all of them.

---

## Command reference

```
studiod studio      serve the interactive studio floor and run what it sends
studiod floor       serve the floor read-only against the existing event log
studiod index       scan a project into its code index and print what moved
studiod asset       draw, build, rig or animate one asset in a project
studiod doctor      check what the studio needs and report what is installed
studiod mcp-server  serve the studio MCP tools over stdio, for a worker
```

Milestone proofs, each of which runs against the real CLI rather than a mock:

```
studiod m1   two same-prefix workers: usage capture, cache reuse, clean reaping
studiod m2   a worker whose only tool is capsule_submit, through the real MCP
studiod m3   Godot end to end through verify and the repair loop
studiod m4   the studio floor driven by a real five-worker cast
```

---

## Repository layout

```
crates/           the Rust workspace — daemon, engine, store, floor
desktop/          the native shell (Windows, macOS, Linux)
installer/        the Windows per-user installer build
packaging/        macOS .app and Linux tarball packaging
.github/          CI and the three-platform release pipeline
docs/design/      19 design documents + 5 ADRs
docs/review/      architecture review findings
agent-images/     the crew reference sheet
probes/           the measurement scripts behind every number quoted here
```

---

## Status

The studio runs. You can create a project, plan a build, watch thirteen roles execute it in parallel, convene meetings that produce durable decisions, verify against a real engine, and commit the result — all from the floor.

Being honest about the edges:

- **Godot is the only engine probed end to end.** Unity and Unreal profiles are written and their command placeholders now resolve, but neither has been run against a real editor.
- **The capsule channel is not yet attached to production workers.** The MCP server, the schema and the trust boundary are all built and tested; wiring them into every spawn is the next piece of work.
- **The control plane has no auth token.** It binds to localhost and rejects cross-origin and rebound-host requests, but any local process can still reach it.
- **macOS and Linux builds are unsigned**, and the Linux desktop shell needs `libwebkit2gtk-4.1`.

The full findings, including what was fixed and what is deliberately still open, are in [`docs/review/`](docs/review/).

---

## Documents

Start with [`docs/design/00-overview.md`](docs/design/00-overview.md) — the full set of 19 design documents and 5 ADRs is indexed there.

<div align="center">
<sub>MIT licensed.</sub>
</div>
