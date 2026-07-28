# Game Studio Crew 1.0.0

A game studio that runs itself. Thirteen AI specialists working a real project in a real engine, on a 3D studio floor you can watch while it happens.

No API keys — everything runs through the coding CLI subscription you already have.

## Downloads

| Platform | File |
|---|---|
| Windows (x86_64) | `game-studio-crew-1.0.0-windows-x86_64.zip` |
| macOS (Apple Silicon) | `game-studio-crew-1.0.0-macos-aarch64.tar.gz` |
| macOS (Intel) | `game-studio-crew-1.0.0-macos-x86_64.tar.gz` |
| Linux (x86_64) | `game-studio-crew-1.0.0-linux-x86_64.tar.gz` |

**Windows** — unzip and run `game-studio.exe`, or build the installer with `installer\build.ps1` for a Start Menu entry.
**macOS** — unpack and move `Game Studio Crew.app` to Applications. The build is unsigned, so the first launch needs right-click → Open.
**Linux** — unpack and run `./install.sh` (installs to `~/.local` by default, no root). Needs `libwebkit2gtk-4.1` for the desktop shell; the daemon alone has no such dependency.

Run `studiod doctor` first on any platform. It tells you what is installed and, separately, what can actually spawn a worker.

## What you get

- **The studio floor** — a top-down 3D office in the browser. Desks light up as workers spawn, characters walk to meetings, a timeline scrubber replays the run, and a workflow track shows the DAG executing.
- **Thirteen roles** across six departments, each with a byte-frozen charter and only the tools its job needs.
- **Editable plans** — the director returns a task graph you can change before anything runs, and guided mode holds after every step for your approval.
- **Meetings that produce decisions** — participants answer each other's positions verbatim, and the chair returns a ruling that lands in the decision store and in the project repo as an ADR.
- **Four built-in workflows** — feature, bugfix, release, sprint planning.
- **Verification against the real engine**, with a repair loop that hands failures back with the exact list.
- **Art the crew draws itself** — sprites, textures, and characters rigged and animated well enough for the engine to load.
- **Version control that costs zero tokens** — the daemon commits, not the workers.
- **Budget governance** — ceilings in tokens or dollars, with graceful degradation instead of a hard stop.
- **A code index** that answers with symbols instead of files, measured at 2.3–3.4× fewer input tokens for the same answer.

## Since the last build

This release is the first that runs on all three desktop platforms, and it closes a long list of correctness gaps found in an architecture review.

**Cross-platform**
- The daemon no longer orphans its worker tree on macOS and Linux. `SIGINT`, `SIGTERM`, `SIGHUP` and `SIGQUIT` now kill every live worker process group before the daemon exits — previously a killed daemon left workers running with no wall clock to stop them, because the thread enforcing it had died with the daemon.
- The desktop shell builds on all three platforms; Windows resource embedding is properly gated.
- macOS `.app` bundles and a Linux tarball with a `.desktop` entry and a rootless installer.

**Runs no longer die quietly**
- A stop no longer leaves its flag set, which used to kill every task and meeting afterwards with no diagnostic until the next build.
- An unanswered approval no longer blocks the daemon for the life of the process; the wait times out, re-announces itself, and can be broken by stop.

**Budget tells the truth**
- Usage accumulates across turns instead of overwriting, so a worker killed at its wall clock is no longer recorded as free — previously the most expensive runs were the ones the budget could not see.
- Failed and killed workers are charged to the run.
- The degradation ladder actually reaches the spawn, and its rung follows budget pressure rather than the number of spawns, so a long run can no longer refuse itself while still solvent.

**Verification cannot be fooled as easily**
- Gate helpers are restored and checked before every gate. A rewritten check fails the gate by name instead of passing silently.
- An empty test suite, a truncated report, an undecided NUnit result and a silent CI helper are all inconclusive now, never a pass.
- `verify()` can no longer hang past its own timeout when a grandchild holds the log pipe open.
- Gates attach to a node that actually runs last, rather than whichever was declared last — which used to compile half-written projects and spend repair workers on code later steps were going to write.
- Infrastructure signatures are matched per line and ignore file names, so a compile error in a file called `licensing.gd` stays a repairable failure instead of halting the run.

**Cost**
- Parallel waves stagger cold prefixes: one worker warms a prefix and the rest follow warm, instead of each paying the cache write premium.

**Engines**
- Unity, Unreal and Godot command placeholders now resolve — export presets, platform names, engine roots and project files are derived rather than left unbound. A scope whose script is missing says which one instead of failing vaguely.

**Durability**
- The resume record is staged, read back and renamed rather than written in place, and progress is recorded after gates and after your verdict, so work you sent back is not resumed as finished.
- SQLite connections carry a busy timeout.

## Known limits

Stated plainly, because you will meet them:

- **Godot is the only engine proven end to end.** Unity and Unreal profiles are written and their commands resolve, but neither has been run against a real editor.
- **The capsule channel is not attached to production workers yet.** The MCP server, schema and trust boundary are built and tested; wiring them into every spawn is the next piece of work.
- **The control plane has no auth token.** It binds to localhost and rejects cross-origin and rebound-host requests, but any local process on your machine can still reach it.
- **macOS and Linux builds are unsigned.**

Full findings, including what was fixed and what is deliberately still open, are in [`docs/review/`](docs/review/).

## Requirements

- A coding CLI. `claude`, `codex`, `gemini`, `copilot` and `kimi` are all detected, but only `claude` can currently run the crew.
- Optional: git, an engine (Godot / Unity / Unreal), and for generated art `node`, a `python` with `pillow`, and the `codex` imagegen skill.

MIT licensed.
