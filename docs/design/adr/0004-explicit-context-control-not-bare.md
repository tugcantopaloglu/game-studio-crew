# ADR 0004: Explicit context control instead of `--bare`

> **Status:** Accepted (M1 probe evidence), 2026-07-20
> Amends the *mechanism* of [ADR 0001](0001-claude-cli-as-worker.md); its decision and its founding constraint are unchanged. Context for [00](../00-overview.md), [01](../01-orchestrator-core.md), [02](../02-context-engine.md).

## Context

[ADR 0001](0001-claude-cli-as-worker.md) commits to `claude` CLI subprocesses billed against the user's **subscription**, with **no API keys**. [00](../00-overview.md) named `--bare` "the primary token lever": the flag that strips `CLAUDE.md`, hooks, skills, plugins and MCP from a worker invocation.

M1 probes ran the flag for the first time. `--bare` reports `Not logged in` and exits in 222 ms having spent zero tokens, with valid, unexpired subscription credentials present on disk. The CLI's own help explains why:

> `--bare` … Anthropic auth is strictly `ANTHROPIC_API_KEY` or `apiKeyHelper` via `--settings` (**OAuth and keychain are never read**).

`--bare` requires an API key. ADR 0001 forbids API keys. **The two cannot both hold**, and the conflict sat undetected in [00](../00-overview.md)'s "verified CLI facts" list because the flag's auth behavior was never probed, only its context-stripping behavior was read from documentation.

## Decision

**Drop `--bare`. Strip context explicitly instead**, with flags that leave OAuth intact:

```
claude -p
  --setting-sources ""                  # no user/project/local settings, no CLAUDE.md discovery
  --system-prompt-file <frozen charter> # replaces the default system prompt entirely
  --tools "<role allowlist>"            # built-in tool schemas: the real token dial
  --allowedTools <role allowlist + mcp__studio__*>
  --mcp-config <orchestrator stdio MCP> --strict-mcp-config
  --permission-mode dontAsk
  --output-format stream-json --include-partial-messages --verbose
```

`--safe-mode` was evaluated as the drop-in replacement and **rejected**: it preserves auth but disables MCP servers unconditionally, and neither `--mcp-config` nor `--strict-mcp-config` overrides that (`init.mcp_servers: []` in every variant probed). Losing MCP would have forced the outbox fallback ([13](../13-risks.md) R1) for no token benefit, because explicit flags reach the same floor without it.

## Evidence

Measured, Opus 4.8, single `say ok` turn, credentials valid throughout:

| Configuration | total input tokens | cost |
|---|---|---|
| default (nothing stripped) | 22572 | $0.2258 |
| `--safe-mode` | 21329 | $0.0517 |
| `--safe-mode --system-prompt` | 19510 | $0.1952 |
| `--safe-mode --system-prompt --tools ""` | 184 | $0.0010 |
| `--bare` (any) | — | **fails: `Not logged in`** |

The decisive line is the third to fourth: replacing the system prompt leaves **19510 tokens**, and emptying the tool list drops it to **184**. The bulk of a default invocation is **built-in tool schemas**, not `CLAUDE.md` or ambient project context. This inverts the causal story [02](../02-context-engine.md) told.

Caching survives the change. Two separate subprocesses, identical frozen prefix, `--tools "Read,Grep,Glob"`:

| | cache_write | cache_read | cost |
|---|---|---|---|
| cold | 8867 | 0 | $0.0888 |
| warm | 0 | 8867 | $0.0051 |

A **17.4×** reduction on the warm invocation, across process boundaries. The token thesis and the cache-fragmentation argument in [ADR 0002](0002-thirteen-roles.md) both hold.

## Consequences

- **The founding constraint is intact.** Subscription billing, no API keys, unchanged. Only the flag that implements context-stripping changed.
- **`--tools` joins the frozen prefix as a cache-identity input.** A role's tool allowlist is part of the cached bytes, so two roles differing only in allowlist mint different prefixes and fragment the cache exactly as two different charters would. [02](../02-context-engine.md) folds the allowlist into the `prefix_hash`; [04](../04-agent-graph.md)'s allowlist table is now a cost surface, not just a permissions surface.
- **The per-role token floor is set by its allowlist.** A coordination-only role (`producer`, `studio_director`, no filesystem tools) runs near the 184-token floor. An engineer carrying Read/Grep/Glob/Edit/Write/Bash pays several thousand tokens per spawn before its charter is counted. Trimming an allowlist is now a budget lever ([06](../06-budget-governance.md)).
- **Two risks close.** MCP attaches under this configuration (`status: "connected"`, tool invoked, value returned), so R1's outbox fallback is not needed; interim `usage` is present on `stream_event`/`message_start`, so R2's EMA fallback is not needed. Both fallbacks stay documented as contingencies, neither is on the build path.
- **`--bare` remains the correct flag for an API-key deployment.** If the no-API-key constraint is ever lifted, this ADR is the thing to revisit, not ADR 0001.

## What this cost us

The design set asserted `--bare` as a verified fact across four documents and an ADR. It was verified as *documented behavior*, never *executed*. The lesson is recorded in [00](../00-overview.md)'s CLI facts section: a fact the architecture rests on is not verified until a probe has run it and the exit code has been read.
