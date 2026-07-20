# 00: System Overview

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> Part of the Game Studio Crew system design set. See [README](../../README.md).

## Prime directive

> **Feed the model minimum viable context, and never pay twice for the same tokens.**

Every design decision in this set is downstream of that sentence. Context is assembled by the daemon, not accumulated in a conversation. Bytes that repeat across invocations are frozen so prompt caching pays for them once. Bytes that don't earn their place are not sent.

## What this is

Game Studio Crew is a re-architecture of `claude-code-game-studios`. The original packs 49 markdown agents, 73 slash commands, 12 hooks and 11 rule files into a **single** Claude Code conversation. Consequences:

- Every invocation reloads `CLAUDE.md` and ambient project context.
- Subagents inherit bloated prompts they can't trim.
- There is no state store, no summarization, no context handoff between steps.
- Token burn is enormous, and the studio is invisible while it works. You get a wall of text, not a view of what the team is doing.

The rebuild is a **Rust daemon** that treats `claude` CLI subprocesses as **stateless workers**. The daemon owns all context, all budget, all state, and streams a realtime event feed to a browser-based visual studio floor. Workers are cheap, disposable, and told only what they need for one task.

- **Tier 1** is the top of the tree: a single `studio_director` seat on **Fable 5**.
- **Tier 2 and Tier 3** (department leads and specialists) run on **Opus 4.8**.

Fable is the more expensive model, not the cheaper one ($10/$50 per MTok against Opus at $5/$25), which is exactly why it sits on the one lowest-volume seat. See [04](04-agent-graph.md).
- **Unity, Unreal Engine 5 and Godot 4** are all first-class.

**Hard constraint: no API keys.** Everything runs through the user's Claude Code **subscription** via the CLI. This is the founding decision. See [ADR 0001](adr/0001-claude-cli-as-worker.md).

## System shape

```mermaid
graph LR
  subgraph Daemon["Rust daemon"]
    ORCH[orchestrator-core<br/>supervisor + token bucket]
    CTX[context-engine<br/>prompt layers + capsules]
    STATE[(state store<br/>SQLite/WAL)]
    IDX[(code+asset index<br/>SQLite/WAL)]
    BUD[budget governance]
    EVT[event bus]
    MCP[MCP server<br/>stdio]
  end
  subgraph Workers["claude CLI workers (stateless)"]
    W1["claude -p --system-prompt-file<br/>--tools --setting-sources ''"]
    W2["claude -p ..."]
  end
  ENG[engine drivers<br/>Unity / UE5 / Godot]
  UI[browser studio floor]

  ORCH --> W1 & W2
  CTX --> ORCH
  STATE <--> ORCH
  STATE <--> CTX
  IDX --> CTX
  BUD --> ORCH
  W1 & W2 -. capsules / tool calls .-> MCP
  MCP --> ORCH
  ORCH --> ENG
  ENG -. structured failures .-> ORCH
  ORCH --> EVT --> UI
  W1 & W2 -. NDJSON stream .-> EVT
```

Workers never talk to each other and never hold durable state. They read a frozen system prompt, receive one volatile task brief, do work, emit a **capsule** through the MCP tool, and exit. The daemon reduces their NDJSON output and MCP calls into events, ledger rows, and state transitions.

## Crate map

Rough Cargo workspace layout the docs assume (names, not a commitment):

| Crate | Owns | Design doc |
|---|---|---|
| `studio-core` | worker supervisor, token bucket, watchdog, process reaping, crash recovery | [01](01-orchestrator-core.md) |
| `studio-context` | layered prompts, charter freezing/hashing, capsules, summarization ladder, symbol index feed | [02](02-context-engine.md) |
| `studio-store` | state SQLite (WAL, single-writer actor), ledger, budgets | [03](03-state-store.md) |
| `studio-agents` | role registry, meeting/delegation/escalation logic | [04](04-agent-graph.md) |
| `studio-events` | event envelope, enum, NDJSON→studio mapping, resume/coalescing | [05](05-event-protocol.md) |
| `studio-budget` | task/sprint budgets, enforcement points, degradation ladder | [06](06-budget-governance.md) |
| `studio-engine` | engine profiles, charter fragments, `EngineDriver` trait | [07](07-engine-layer.md) |
| `studio-verify` | `verify()` contract, per-engine report parsers, repair loop | [08](08-verification.md) |
| `studio-workflow` | TOML DAG workflows, node/edge/gate execution | [09](09-workflows.md) |
| `studio-standards` | rule modes (lint/check/prompt), R0-R4 trust model | [10](10-standards-and-trust.md) |
| `studio-index` | engine detection, index SQLite, tree-sitter extractors, watcher | [11](11-index-and-bootstrap.md) |
| `studio-ui` | browser studio floor (separate frontend build) | [12](12-visual-workspace.md) |

Two **separate** SQLite databases: the **state store** ([03](03-state-store.md)) holds runtime state; the **index** ([11](11-index-and-bootstrap.md)) holds the code/asset map. They are never the same file and never share a connection pool.

## Verified CLI facts the design rests on

**Measured by the M1 probes ([`probes/`](../../probes/README.md)), not read from documentation.** Every doc that leans on the CLI cites this list rather than re-deriving it. A fact here is not "verified" until a probe has executed it and its exit code has been read; the `--bare` reversal below is what that rule was written from.

- `claude -p --output-format stream-json --include-partial-messages --verbose` streams NDJSON: `system`/`init`, `stream_event` (`message_start`, `content_block_start`/`_delta`/`_stop`, `message_delta`/`_stop`), `assistant`, `rate_limit_event`, and a terminal `result`. `stream-json` **requires `--verbose`** or the CLI errors out.
- **`--bare` is unusable here.** It reads auth *strictly* from `ANTHROPIC_API_KEY` or `apiKeyHelper`; OAuth and keychain are never read, so it fails `Not logged in` against a subscription. Context is stripped explicitly instead. See [ADR 0004](adr/0004-explicit-context-control-not-bare.md).
- **The primary token lever is `--tools`.** Built-in tool schemas dominate a default invocation: replacing the system prompt alone leaves ~19.5k input tokens; emptying the tool list drops the same call to **184**. `--setting-sources ""` suppresses settings and `CLAUDE.md` discovery.
- `--system-prompt-file` replaces the system prompt entirely. `--model` accepts `fable`, `opus` and `haiku`. `--effort low|medium|high|xhigh|max`.
- `--session-id`, `--resume`, `--fork-session`. Sessions persist as JSONL under `~/.claude/projects/<slug>/`; lookup is scoped to the working directory.
- The terminal `result` carries `usage` (`input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`) plus `total_cost_usd`, `modelUsage`, `session_id` and `terminal_reason`.
- Prompt caching is automatic, keyed on **exact system-prompt bytes + tool set + model**. Identical prefixes across separate subprocesses **do** hit cache: measured **8867 tokens written cold, 8867 read warm, a 17.4× cost reduction**. The tool allowlist is part of the cached identity, not just the charter.
- Cache TTL is **1 hour** (`cache_creation.ephemeral_1h`), not 5 minutes. The write premium is **2.0× base**, measured exactly ($0.0888 against a $0.0443 uncached baseline), not the 1.25× that a 5-minute TTL would cost.
- `--permission-mode dontAsk` with `--allowedTools` runs fully non-interactive. Stdin must be redirected explicitly or the CLI waits on it.
- A Rust process can serve **MCP over stdio**, and `--mcp-config --strict-mcp-config` attaches it under the configuration above (`status: "connected"`, tool invoked, value returned). `--safe-mode` does **not** attach MCP under any variant probed.

### The two formerly-unverified behaviors (settled in M1)

Both resolved in the favorable direction. Their fallbacks remain documented in [13](13-risks.md) as contingencies but are **not on the build path**:

1. **Does `--mcp-config` attach?** **Yes**, given [ADR 0004](adr/0004-explicit-context-control-not-bare.md)'s flag set. The watched-outbox fallback is not needed. (It does *not* attach under `--safe-mode`, which is why `--safe-mode` was rejected.)
2. **Do streamed events carry usable interim `usage` deltas?** **Yes.** `stream_event`/`message_start` carries a full `usage` block, and four pre-`result` events carry usage in a short turn. In-flight budgeting reads real numbers; the EMA fallback is not needed. See [06](06-budget-governance.md).

## Milestone order

- **M0 (complete):** design documents only. Reviewed and iterated before any code.
- **M1 (in progress):** CLI probes **done**, all three verdicts settled ([`probes/`](../../probes/README.md), [ADR 0004](adr/0004-explicit-context-control-not-bare.md)). Remaining: supervisor + state store + ledger. Spawn one worker; prove (a) usage capture from `result`, (b) cache hit on a second same-role spawn within the TTL, (c) clean process reaping on Windows. **The probes existed specifically to settle the unverified CLI behaviors** before anything else was built on them, and they overturned one.
- **M2:** context engine: frozen charters, capsules, summarization ladder.
- **M3:** one engine end-to-end (Godot first: fully headless, no editor lock) through verify + repair loop.
- **M4:** event protocol + minimal studio floor (avatars, status rings, event feed).
- **M5:** workflows, budget governance, remaining engines, full visual workspace.

## Reading order

Start here, then [02 context-engine](02-context-engine.md) for the token story, then [04 agent-graph](04-agent-graph.md) for the roles. [13 risks](13-risks.md) is the honest list of what could break.

## Document set

| # | Doc | Owns (single source of truth for) |
|---|---|---|
| 00 | overview | crate map, CLI facts, milestones, prime directive |
| 01 | [orchestrator-core](01-orchestrator-core.md) | worker lifecycle, supervision |
| 02 | [context-engine](02-context-engine.md) | prompt layers, capsule schema, summarization, token math |
| 03 | [state-store](03-state-store.md) | **state** SQLite schema, ledger |
| 04 | [agent-graph](04-agent-graph.md) | **the 13-role registry** |
| 05 | [event-protocol](05-event-protocol.md) | **event envelope + enum** |
| 06 | [budget-governance](06-budget-governance.md) | budget model, degradation ladder |
| 07 | [engine-layer](07-engine-layer.md) | **engine profile schema + 3 profiles** |
| 08 | [verification](08-verification.md) | `verify()` contract, report parsers |
| 09 | [workflows](09-workflows.md) | workflow DAG schema |
| 10 | [standards-and-trust](10-standards-and-trust.md) | rule modes, R0-R4 trust |
| 11 | [index-and-bootstrap](11-index-and-bootstrap.md) | **index** SQLite schema, detection |
| 12 | [visual-workspace](12-visual-workspace.md) | studio floor, event→visual mapping |
| 13 | [risks](13-risks.md) | consolidated risk register |
| ADR | [0001](adr/0001-claude-cli-as-worker.md) · [0002](adr/0002-thirteen-roles.md) · [0003](adr/0003-top-down-not-isometric.md) · [0004](adr/0004-explicit-context-control-not-bare.md) | decision records |
