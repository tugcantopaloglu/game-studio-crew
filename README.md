# Game Studio Crew

A ground-up rebuild of `claude-code-game-studios` as a **Rust daemon** that drives `claude` CLI subprocesses as stateless workers, owns all context and budget itself, and streams a realtime event feed to a browser-based visual studio floor.

> **Status:** system-design phase. This repository currently contains **design documents only**: no runtime code. See [`docs/design/`](docs/design/00-overview.md).

## The problem

The original crew packs **49 agents, 73 slash commands, 12 hooks and 11 rule files** into a single Claude Code conversation. Every invocation reloads `CLAUDE.md` and ambient context, subagents inherit bloated prompts, and there is no state store, no summarization, and no handoff between steps. Token burn is enormous, and the studio is invisible while it works.

## How this differs

| | Original | This rebuild |
|---|---|---|
| Shape | one long conversation | Rust daemon + stateless CLI workers |
| Context | accumulated in the chat | assembled per-task by the daemon |
| Prompts | reloaded every turn | frozen, content-hashed, cache-warm charters |
| Inter-agent comms | shared conversation | schema-validated **capsules** only |
| State | none | two SQLite stores (runtime + code/asset index) |
| Roles | 49 (triplicated per engine) | **13** (engine is a prompt layer, not a role axis) |
| Visibility | wall of text | realtime top-down studio floor |
| Billing | n/a | **Claude Code subscription, no API keys** |

## The three-engine story

**Unity, Unreal Engine 5 and Godot 4** are all first-class. An engine is a *prompt layer plus a driver*, not a fork of the whole crew. Each engine ships a profile (build/test/import/export command lines) and prose fragments injected into charters; the same 13 roles operate all three. See [`07-engine-layer.md`](docs/design/07-engine-layer.md).

## The token thesis

> **Feed the model minimum viable context, and never pay twice for the same tokens.**

- `--bare` strips auto-loaded `CLAUDE.md`, hooks, skills, plugins and MCP. The primary token lever.
- Charters are byte-frozen and content-hashed so **prompt caching** (5-min TTL, keyed on exact system-prompt bytes) pays for them once and every same-role worker within the window reads from cache.
- **13 roles, not 49**: fewer distinct prefixes means hotter caches.
- A three-rung summarization ladder distilled by the daemon at **zero token cost** keeps briefs small.

Estimated effect: roughly **40k → 10k input tokens per invocation**, most of the remainder being cache reads. These are estimates; the token ledger replaces them with measurements. See [`02-context-engine.md`](docs/design/02-context-engine.md).

## Constraint: no API keys

Everything runs through the user's Claude Code **subscription** via the `claude` CLI. There is no Messages API usage and no key management. See [ADR 0001](docs/design/adr/0001-claude-cli-as-worker.md).

## Documents

Start with [`docs/design/00-overview.md`](docs/design/00-overview.md). The full set (14 design docs + 3 ADRs) is indexed there.
