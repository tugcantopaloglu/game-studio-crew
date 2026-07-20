# ADR 0001: Claude CLI subprocesses as stateless workers

> **Status:** Accepted (design phase), 2026-07-20. **Mechanism amended by [ADR 0004](0004-explicit-context-control-not-bare.md)** after M1 probes: `--bare` is incompatible with subscription auth and has been dropped. The decision and the no-API-key constraint below are unchanged.
> Context for [00](../00-overview.md), [01](../01-orchestrator-core.md), [02](../02-context-engine.md).

## Context

The studio needs to run Claude across many role invocations. Two surfaces are available: the **Messages API** (HTTP, API-key billed) and the **`claude` CLI** (subprocess, subscription billed). The founding constraint is **no API keys**: everything runs through the user's Claude Code **subscription**.

## Decision

Drive Claude by spawning **`claude` CLI subprocesses as stateless workers**, one per task, supervised by the daemon ([01](../01-orchestrator-core.md)). The daemon owns all context, state, and budget; the worker is disposable.

The CLI facts this rests on ([00](../00-overview.md), now probe-measured): `-p --output-format stream-json --include-partial-messages --verbose` for structured NDJSON; `--setting-sources ""` plus `--system-prompt-file` plus `--tools` to strip ambient loading and control the prefix ([ADR 0004](0004-explicit-context-control-not-bare.md), replacing `--bare`); `--model fable|opus` and `--effort`; `--session-id`/`--resume`/`--fork-session` with JSONL persistence under `~/.claude/projects/<slug>/`; `--permission-mode dontAsk --allowedTools` for non-interactive runs; a terminal `result` carrying `usage`/`total_cost_usd`/`modelUsage`; a Rust process serving MCP over stdio for callbacks, attached with `--mcp-config --strict-mcp-config`.

## Why not the Messages API

- **It requires API keys and API billing.** That violates the founding constraint outright. The user pays via subscription, not a metered key.
- Key management, rotation, and per-org quotas would all be new surface area the subscription model avoids.

## What we give up

- **No direct control over caching internals, rate limits, or model routing.** These are the CLI's/subscription's, not ours, hence R3 (opaque limits) and R4 (cache opacity) in [13](../13-risks.md), mitigated by the AIMD bucket and the `cache_hit_ratio` metric rather than by direct control.
- **Two CLI behaviors were unverified**: MCP attachment and interim usage deltas. M1 settled both favorably ([13](../13-risks.md) R1, R2); neither fallback is built. A third, unlisted assumption, that `--bare` works at all under subscription auth, turned out false ([ADR 0004](0004-explicit-context-control-not-bare.md)).
- **Subprocess overhead and OS-level reaping complexity**: mitigated by Windows Job Object reaping and crash recovery ([01](../01-orchestrator-core.md)); a cost we accept for subscription billing.
- **Coarser telemetry** than the API would give: we reconstruct it from NDJSON + the terminal `result` into the ledger ([03](../03-state-store.md)).

## Consequences

The stateless-worker model *forces* the rest of the architecture in a good direction. Because the worker keeps nothing, the daemon must own context ([02](../02-context-engine.md)), state ([03](../03-state-store.md)), and budget ([06](../06-budget-governance.md)), which is exactly the discipline that makes the token story work. The constraint and the architecture reinforce each other.
