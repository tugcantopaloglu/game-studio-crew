# Game Studio Crew

A ground-up rebuild of `claude-code-game-studios` as a **Rust daemon** that drives `claude` CLI subprocesses as stateless workers, owns all context and budget itself, and streams a realtime event feed to a browser-based visual studio floor.

> **Status:** M1 through M5 built and running against the real CLI and a real engine; M6, the code index, is under way. `studiod studio` serves an interactive 3D studio floor: you assign tasks, convene meetings and start workflows from the browser, and watch real `claude` workers do them. **Godot is the only probed engine**; the Unity and UE5 profiles are written but have never been executed ([07](docs/design/07-engine-layer.md)).

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

- **`--tools` is the primary token lever.** Built-in tool schemas, not `CLAUDE.md`, are the bulk of a default invocation: the same call costs **22572** tokens with the default tool set and **184** with an empty one.
- Charters are byte-frozen and content-hashed so **prompt caching** (1-hour TTL, keyed on exact system-prompt bytes plus tool set) pays for them once and every same-role worker within the window reads from cache.
- **13 roles, not 49**: fewer distinct prefixes means fewer cold starts, and a cold start costs a **2.0×** write premium.
- A three-rung summarization ladder distilled by the daemon at **zero token cost** keeps briefs small.
- **Symbols, not files.** A tree-sitter index answers `symbol_lookup` with a signature, doc comment and one-hop neighbourhood, so a worker learns that `Enemy.attack` calls `Player.take_damage` without either file entering its context ([11](docs/design/11-index-and-bootstrap.md)).

Measured effect: a warm invocation's prefix costs **$0.0051 against $0.0888 cold and $0.2258 undefended, a 17.4× warm-to-cold reduction**, across separate subprocesses. These are M1 probe measurements, not estimates. See [`02-context-engine.md`](docs/design/02-context-engine.md) and [`probes/`](probes/README.md).

## Constraint: no API keys

Everything runs through the user's Claude Code **subscription** via the `claude` CLI. There is no Messages API usage and no key management. See [ADR 0001](docs/design/adr/0001-claude-cli-as-worker.md).

This constraint is load-bearing enough that it killed the design's original token lever: `--bare` reads auth strictly from an API key, so it fails against a subscription. Context is stripped explicitly instead, which reaches a lower floor anyway. See [ADR 0004](docs/design/adr/0004-explicit-context-control-not-bare.md).

## Documents

Start with [`docs/design/00-overview.md`](docs/design/00-overview.md). The full set (14 design docs + 4 ADRs) is indexed there.
