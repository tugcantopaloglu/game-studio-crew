# 18: Desktop Shell, Install and Crash Reporting

> **Status:** v0.3, 2026-07-25. Built, run and measured on Windows 11: the shell starts its own daemon and serves the floor **1.60 s** after launch, kills the daemon on window close, attaches to a daemon that is already running, and the installer was compiled, installed and uninstalled on this machine. `studiod doctor` reports against the real toolchain and separates *installed* from *the studio can drive it*; the panic hook writes a redacted report. **Not exercised here:** any platform other than Windows.
> **Consumes** the studio server ([12](12-visual-workspace.md)) and the process group from `studio-core` ([01](01-orchestrator-core.md)). Owns `desktop/`, `installer/`, `crates/studio-server/src/health.rs`, `crates/studiod/src/doctor.rs` and `crates/studiod/src/crash.rs`.

## What the shell is

The floor is already a local web app served by the daemon on `127.0.0.1:7878`. The desktop app is therefore not a rewrite of the floor; it is **a native window around it plus a supervisor for the daemon**. It does three things a browser tab cannot: it starts the daemon for you, it tells you plainly when the daemon dies, and it takes the daemon and every worker down with it when you close the window.

It lives in `desktop/`, a crate with its **own `[workspace]` table and its own lockfile**, so it is not a member of the root workspace and `cargo test --workspace` never has to fetch a GUI toolchain. It depends on `studio-core` by path for one thing only: `ProcessGroup`, the Job-object process tree killer that the supervisor already uses.

### Why wry + tao and not Tauri

Both were built here. A minimal Tauri v2 app compiles on this machine and produces an **8.28 MB** binary from **419** locked crates on the default release profile. The wry + tao shell produces a **576 KB** binary from **282** locked crates with LTO, one codegen unit, `panic = "abort"` and symbols stripped. Tauri's value is its command bridge, its bundler and its updater; the floor talks to the daemon over HTTP and a WebSocket and needs none of them, so all three would be paid for and unused. The shell is 60 lines of window code around a WebView, which is exactly the amount of framework the job justifies.

### Why the daemon stays a separate process

It could have been a library linked into the window. It is not, for four reasons that all point the same way:

- **The daemon outlives and predates the window.** `studiod studio` from a terminal is still the primary way to run the studio, and the M-proofs depend on it. A shell that only works when linked in would fork the product in two.
- **A GUI crash must not take the crew with it.** WebView2 lives in the window process. Keeping the orchestrator out of that process means a renderer fault loses a view, not a run.
- **Killing the tree is already solved for a child process.** `ProcessGroup` assigns the daemon to a Job object with `KILL_ON_JOB_CLOSE`; the daemon's workers are its own children and inherit the job. Closing the window is therefore a guaranteed full stop, including a `claude` that is mid-turn. **Every** process the shell starts goes into that one job, the requirements check included — the first live run orphaned the doctor process on close and it panicked on a stdout pipe nobody was reading any more, which is exactly the class of mess the job exists to prevent.
- **One window is not the only client.** The floor is reachable from an ordinary browser at the same time, which is how you watch a run from a second screen.

### Startup, in order

1. **Probe the port first.** If `127.0.0.1:7878` already answers, the shell **attaches** to that daemon rather than starting a second one. An attached daemon is never killed on window close — you did not start it, so the shell does not stop it.
2. Otherwise locate `studiod` next to the shell binary, then on `PATH`.
3. Resolve the studio home: `STUDIO_HOME`, else `%LOCALAPPDATA%\GameStudioCrew`. The daemon derives `.studio/` from its working directory, so the shell runs it there. **Projects are unaffected** — they live at the absolute paths you chose and are never moved under the app.
4. Start `studiod studio` as a child with stdout and stderr redirected to `.studio/daemon.log` (truncated each start), and adopt it into the Job object.
5. **Wait for the port to answer, not for a fixed sleep.** The window shows a splash until the floor responds, polling every 100 ms up to 45 s and checking on every poll whether the child has exited.
6. Concurrently run the requirements check (below). It does not gate the floor: it only replaces the page if the answer is "nothing to code with", so a healthy machine never waits on it.

If the daemon exits before it serves, or dies later while the window is open, the shell replaces the page with the last lines of `daemon.log` and says what to do next. All daemon text is HTML-escaped on the way into that page.

## The requirements check

One probe, two renderings. `studio_server::health::probe()` produces the data; `studiod doctor` formats it for a terminal and `GET /health` serves the identical structure as JSON for the shell and the settings panel. Adding a tool in one place cannot leave the other stale, because there is only one place.

It reports, present or absent with the version it found:

| Group | Probed | Required |
|---|---|---|
| coding CLIs | `claude`, `codex`, `gemini`, `copilot`, `kimi` | **at least one installed** |
| toolchain | `git`, `cargo`, `rustc` | no |
| engines | every builtin engine profile ([07](07-engine-layer.md)), resolved through `studio_engine::resolve_binary` | no |

Only the coding CLIs are load-bearing. Probes run in parallel with a 3 s cap each, which is what keeps the whole check to about four seconds on this machine rather than eighteen. Unity and Unreal are resolved but never executed for a version string — those editors are not safe to launch to print one line — so they read as `present` without a version.

### Installed and drivable are two different facts

A CLI being on `PATH` does not mean the studio can spawn a worker through it. `gemini` and `copilot` have no flag that replaces the system prompt, so a frozen charter cannot be delivered and the studio refuses to spawn through them; `kimi`'s flags have never been read; `codex` has no provider at all. Today `claude` is the only CLI that can drive the crew.

The doctor reports **both** facts, and never lets the first pass for the second. Each installed CLI is tagged either *the studio spawns workers through this* or *installed, but the studio cannot drive it*, and an install where nothing is drivable prints the per-CLI reason and says to install Claude Code. This exists so nobody installs, sees a tick next to "gemini found", and discovers at the first task that no worker can start.

The verdict is **not this module's opinion**. `drivable` is `studio_core::Provider::can_serve(RoleNeeds::default())` and the reason text is the first line of `Provider::blockers`, so the doctor, the settings panel and the supervisor cannot disagree. A test asserts row by row that the report agrees with `Provider::ALL`. **If the provider table gains a CLI or a capability, this report follows it automatically; the only thing that must be kept in step by hand is the `CODING_CLIS` probe list, which is what decides whether a CLI is looked for at all.**

**Exit codes:** `0` ready and something can spawn, `2` nothing to code with, `3` a coding CLI is installed but the studio can drive none of them. The installer shows the doctor's own output on `2` and on `3`, with a different headline for each. The shell blocks the floor only on `2`: per the user's decision an install with only Gemini is a successful install, so the window opens and the settings panel carries the drivability report rather than the app refusing to start.

## What the installer touches

`installer/game-studio-crew.iss` is an Inno Setup 6 script; `installer/build.ps1` is the one-command recipe (`powershell -ExecutionPolicy Bypass -File installer\build.ps1`) that builds the daemon, builds the shell, and compiles the installer into `installer/out/`.

It is a **per-user install** — `PrivilegesRequired=lowest` — which means no UAC prompt, no admin, and nothing written outside the user's own profile:

- `%LOCALAPPDATA%\Programs\Game Studio Crew\` — `game-studio.exe`, `studiod.exe`, the uninstaller.
- `%APPDATA%\...\Start Menu\Programs\Game Studio Crew.lnk`.
- One uninstall key under `HKCU\...\Uninstall`.

It does **not** touch `PATH`, register a service, install a scheduled task, associate a file type, or write to `HKLM`. Before installing it checks for the WebView2 runtime and says plainly what an empty window would mean if it is missing.

Uninstalling removes all three of the above. It then **asks** whether to remove `%LOCALAPPDATA%\GameStudioCrew` — the event log, the decision store, the daemon log and any crash reports — and says in the prompt that project folders live wherever you put them and are never touched. A silent uninstall never deletes data it was not explicitly asked to delete.

## What a crash report contains, and what it must not

`crates/studiod/src/crash.rs` installs a panic hook as the first statement in `main`. On a panic it composes a report, **redacts it**, writes it to `.studio/crashes/crash-<timestamp>.txt` (falling back to the temp directory), and then asks.

It contains: the app version, the OS and its build string, which subcommand was running, the time, the panic message and its source location, the backtrace, and the last lines recorded through `crash::note`.

It does not contain: **any absolute path from the machine that produced it**, the user's name, the contents of a project, a task brief, or anything a worker wrote. Redaction rewrites every absolute path down to its file name — `...\crates\studiod\src\studio.rs:412` becomes `<path>/studio.rs:412`, which keeps the only part of a backtrace worth reading — then scrubs the home directory and the user name from whatever is left. A test asserts that no absolute path survives in a report generated from this machine's own working directory.

**The daemon's stdout is deliberately not tailed into the report.** `studiod studio` prints task briefs, decision claims and worker output as it works; no redaction pass can reliably tell a project's plot summary from a log line, so the safe move is to not read that file at all. The report's tail comes only from `crash::note`, which records lifecycle lines and truncates each to 200 characters. Today that is the invocation line; anything the daemon later chooses to record goes through the same cap and the same redaction.

The redaction was confirmed by an accident rather than only by its tests. The first live run of the shell orphaned the doctor process, which panicked writing to a closed pipe, and the hook wrote a real report unprompted. It carried the version, `Microsoft Windows [Version 10.0.26200.8875]`, `subcommand: studiod doctor`, the backtrace and the lifecycle line — and **no path from this machine**. The one path in it was rustc's own virtual sysroot, which is not user data. It also showed a limit worth naming: the release binary carries no debug info, so most backtrace frames read `<unknown>`. The report still identifies the panic and where it came from, but symbol names are a `debug = 1` build away and are not there today.

**Filing is a question, never an action.** Nothing is posted anywhere. There is no token, no API key, and no silent upload. On a terminal the hook asks; on `y` it opens a prefilled GitHub issue URL in the browser, and on anything else the file simply stays on disk. When stdin is not a terminal it prints the issue URL and returns. The target repository defaults to `tugcantopaloglu/game-studio-crew` and is overridable with `STUDIO_CRASH_REPO`.

## Measured on this machine

Windows 11 Pro 26200, rustc 1.97.1, MSVC.

| Thing | Measured |
|---|---|
| shell binary, tuned release | **576 KB** (589,824 bytes) |
| Tauri v2 equivalent, default release | 7.90 MB (8,284,160 bytes) |
| `studiod.exe`, workspace release profile | 18.4 MB (19,344,384 bytes) |
| installer | 4.8 MB |
| installed footprint | **23.26 MB** (24,394,759 bytes, uninstaller included) |
| `studiod doctor`, serial probes | 17.8 s |
| `studiod doctor`, parallel probes | 4.2 s |
| shell launch to the floor answering | **1.60 s and 1.70 s**, two runs |

That is from process start to `127.0.0.1:7878` accepting a connection, with the requirements check running beside it as a second child rather than in front of it — both children were visible under the shell's PID during the run, and both were gone with the port free within seconds of the window closing. The window is up and showing the splash for most of it.

One operational note the run turned up: the daemon inherits the shell's environment, so launching the app from inside a Claude Code session makes `guard_nested_session` refuse to start and the window shows that refusal verbatim. That is the guard working, not the shell failing; started normally from the Start Menu there is no such variable.

The daemon is thirty times the size of the shell because it carries SQLite, tree-sitter grammars and tokio, and because the root workspace's release profile is untuned — deliberately not changed here, since every crate in the repo shares it.

## What is not built

- **Windows only.** The shell compiles for other platforms in principle and `ProcessGroup` has a POSIX path, but nothing here has been run on macOS or Linux, and the installer is Inno Setup.
- **No app icon.** Shortcuts use the default executable icon.
- **No auto-update.** Installing a new version over an old one works; nothing checks for one.
- **Only the happy path of the daemon-died page was seen.** The failure page was rendered from a daemon that refused to start; a daemon that dies *after* the floor has loaded is watched for on a 500 ms poll but was never made to happen deliberately.
