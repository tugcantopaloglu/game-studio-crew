<img src="images/logo-tile.png" alt="Game Studio Crew" width="104">

# Game Studio Crew

A ground-up rebuild of `claude-code-game-studios` as a **Rust daemon** that drives `claude` CLI subprocesses as stateless workers, owns all context and budget itself, and streams a realtime event feed to a browser-based visual studio floor.

> **Status:** M1 through M5 built and running against the real CLI and a real engine; M6, the code index, is under way. `studiod studio` serves an interactive 3D studio floor: you create a project, assign tasks, convene meetings and start workflows from the browser, and watch real `claude` workers do them. **Godot is the only probed engine**; the Unity and UE5 profiles are written but have never been executed ([07](docs/design/07-engine-layer.md)).

## The desktop app

There is a Windows desktop build. `powershell -ExecutionPolicy Bypass -File installer\build.ps1` compiles the daemon, the shell and a per-user installer in one command; the installer needs no admin rights, adds a Start Menu entry, touches neither `PATH` nor any service, and on uninstall asks before removing the studio's own data — your project folders live where you put them and are never touched. The window itself is a thin native frame around the same floor the daemon already serves on `127.0.0.1:7878`: it starts `studiod studio` for you, waits for the port rather than guessing, and takes the daemon and every worker down with it when you close it. If a daemon is already running it attaches to that one instead of starting a second, and leaves it running when the window closes. See [`18-desktop-shell.md`](docs/design/18-desktop-shell.md).

What it requires is one coding CLI and nothing else, and `studiod doctor` is careful to tell you two different things about them: which ones you have, and which ones the studio can actually spawn a worker through. `claude`, `codex`, `gemini`, `copilot` and `kimi` are all looked for and **any one of them installed is enough for the install to succeed** — but only a CLI that can be handed a frozen charter can run the crew, which today means `claude` alone ([04](docs/design/04-agent-graph.md)). So an install that finds only Gemini completes, and then says plainly that nothing installed can start a worker yet and what to install to fix it, instead of showing you a tick you would find out the truth about at your first task. git, cargo and the engines are reported the same way, present or absent with the version found, and their absence is never an error. There are still no API keys anywhere in this: the CLI you already pay for is the whole authentication story.

One capability needs more than that, and the doctor now says so rather than letting you find out at your first art task. The crew can **draw its own art**: it asks `codex` for a sprite or a texture, keys the flat background out of it, and turns a character into a rigged, animated model the engine loads ([19](docs/design/19-asset-generation.md)). That path wants `node`, a `python` that can import `pillow`, and the imagegen skill `codex` ships — none of which the studio installs for you, and all of which it now reports by name, one line each, with the exact command that fixes it. It is optional in the honest sense: with any of it missing every other path behaves exactly as before and the crew builds art by hand. `studiod doctor --fix` will run the installs it can after showing you the list and asking; the one step it cannot do for you is `codex login`, and it says so.

## Projects

Work is scoped to a project: a directory you name, at a path you choose, with its own git repo. Create one from the floor — name, absolute path, engine, and whether to `git init` — and every task, workflow and build you send afterwards runs with that directory as the worker's working directory and indexes against that tree.

The daemon commits for you. After each worker completes, it stages the project and writes a commit subject of `<role>: <first line of the brief>`, so history reads as ordinary studio work and you can diff or revert any single worker's output. **Committing is daemon work, not worker work:** no worker ever runs git or receives git tools, and no commit message is model-generated, so version control costs zero tokens.

Nothing runs without a project selected. Earlier builds fell back to the daemon's own working directory, which meant the crew could edit this repo; that fallback is now an error.

## Meetings produce decisions, not transcripts

Convene a room from the floor and the daemon runs it as a real deliberation. Each participant after the first is handed the previous positions **verbatim** and asked to answer them; the chair — the nearest common ancestor in the escalation tree — receives the whole room and is held to a schema, so it returns a rule the studio will follow, the reason it beat the alternative, and the positions it overrules.

The ruling is durable in two places. It goes into the decision store, so `decision_search` can hand it to a worker weeks later, and into `docs/decisions/` in the project repo as an ADR that records the claim, the reasoning, the dissent and the whole room — committed daemon-side, so version control still costs zero tokens. A chair that returns nothing usable adjourns the meeting; the studio never records a decision nobody made.

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
- **Symbols, not files.** A tree-sitter index answers `symbol_lookup` with a signature, doc comment, one-hop neighbourhood and the scene node the script is mounted on, so a worker learns that `Enemy.attack` calls `Player.take_damage` and that it runs on the `CharacterBody2D` at `Player` in `scenes/main.tscn` — without any of those files entering its context ([11](docs/design/11-index-and-bootstrap.md)). Measured against a worker with `Read,Grep,Glob` and no index on the same question: **2.3-3.4× fewer input tokens**, same correct answer ([`probes/`](probes/README.md)).

Measured effect: a warm invocation's prefix costs **$0.0025 against $0.0374 cold, a 14.8× warm-to-cold reduction**, across separate subprocesses, against **$0.2258 undefended**. These are M1 probe measurements, not estimates, re-taken after the charter padding dropped to each model's real minimum cacheable prefix plus a quarter — the earlier and widely-quoted $0.0051 / $0.0888 / 17.4× were the same probe against a charter padded to 4096 tokens, which the studio no longer builds. The 2.0× cache-write premium survived the change unchanged. One thing the re-measure settled: the charter is the *small* part of a cached prefix — 651 estimated charter tokens cache as 3666 real ones, because the prefix is the charter plus the tool schemas. See [`02-context-engine.md`](docs/design/02-context-engine.md) and [`probes/`](probes/README.md).

## Constraint: no API keys

Everything runs through the user's Claude Code **subscription** via the `claude` CLI. There is no Messages API usage and no key management. See [ADR 0001](docs/design/adr/0001-claude-cli-as-worker.md).

This constraint is load-bearing enough that it killed the design's original token lever: `--bare` reads auth strictly from an API key, so it fails against a subscription. Context is stripped explicitly instead, which reaches a lower floor anyway. See [ADR 0004](docs/design/adr/0004-explicit-context-control-not-bare.md).

## Documents

Start with [`docs/design/00-overview.md`](docs/design/00-overview.md). The full set (19 design docs + 5 ADRs) is indexed there.
