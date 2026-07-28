# Game Studio Crew 1.0.1

A game studio that runs itself. Thirteen AI specialists working a real project in a real engine, on a 3D studio floor you can watch while it happens.

No API keys — everything runs through the coding CLI subscription you already have.

## Install in one line

**Windows** (PowerShell)
```powershell
irm https://raw.githubusercontent.com/tugcantopaloglu/game-studio-crew/main/scripts/install.ps1 | iex
```

**macOS and Linux**
```bash
curl -fsSL https://raw.githubusercontent.com/tugcantopaloglu/game-studio-crew/main/scripts/install.sh | sh
```

Each script detects your platform, pulls the matching build from this release and installs it without admin or root. `install.ps1 -Portable` unpacks instead of installing; `PREFIX=/usr/local` installs system-wide on Linux.

## Downloads

| Platform | Installer | Archive |
|---|---|---|
| Windows (x86_64) | `game-studio-crew-1.0.1-setup.exe` | `game-studio-crew-1.0.1-windows-x86_64.zip` |
| macOS (Apple Silicon) | `game-studio-crew-1.0.1-macos-aarch64.dmg` | `game-studio-crew-1.0.1-macos-aarch64.tar.gz` |
| macOS (Intel) | `game-studio-crew-1.0.1-macos-x86_64.dmg` | `game-studio-crew-1.0.1-macos-x86_64.tar.gz` |
| Linux (x86_64) | `install.sh` inside the tarball | `game-studio-crew-1.0.1-linux-x86_64.tar.gz` |

**Windows** — the setup is per-user: no admin rights, no `PATH` changes, no service. On uninstall it asks before removing the studio's own data.
**macOS** — open the `.dmg` and drag the app to Applications. The build is unsigned, so the first launch needs right-click → Open.
**Linux** — unpack and run `./install.sh` (installs to `~/.local` by default, no root). Needs `libwebkit2gtk-4.1` for the desktop shell; the daemon alone has no such dependency.

## What changed in 1.0.1

**The Windows installer asks instead of reports.**

1.0.0 finished by dumping the whole requirements report into a message box — a wall of text with one OK button, and a single yes/no that either ran every missing art step or none of them.

It is a wizard page now:

- Every tool the studio looks for is its own row, ticked if the studio can already use it.
- Anything installable is a **checkbox you decide on one at a time**, so you can take `codex` and skip `pillow`, or the reverse.
- `claude` and `codex` arrive **already ticked when they are missing** — one runs the crew, the other draws for it.
- Clicking a row explains what it is and prints the **exact command** the tick will run, so nothing installs behind your back.
- Toolchain and engines collapse to a line each; there is nothing to tick there.

The page is drawn from `studiod doctor --porcelain`, a new machine-readable mode, so detection lives in one place. The installer knows exactly what `doctor` knows and cannot drift from it.

**The app starts wherever you installed it from.** The desktop shell was handing the daemon its own session markers. A window that starts a daemon is not a nested CLI session, but the marker was inherited, so installing from inside a Claude Code terminal produced an app that refused to start with `refusing to run inside a Claude Code session`. The shell clears those markers now.

**Smaller fixes.** Coding CLIs carry install commands, so all five are tickable rather than only the art pipeline. The requirements page is laid out in scaled units, so it does not overlap itself on a display with scaling turned up, and long text can no longer run off the bottom of the page.

If the crew stands still on the floor, that is the **crew motion** setting: it defaults to following the system, and Windows' "Animation effects" being off reads as *reduce motion*. Set crew motion to `on` in settings to override it.

## Known limits

Stated plainly, because you will meet them:

- **Godot is the only engine proven end to end.** Unity and Unreal profiles are written and their commands resolve, but neither has been run against a real editor.
- **The capsule channel is not attached to production workers yet.** The MCP server, schema and trust boundary are built and tested; wiring them into every spawn is the next piece of work.
- **The control plane has no auth token.** It binds to localhost and rejects cross-origin and rebound-host requests, but any local process on your machine can still reach it.
- **macOS and Linux builds are unsigned**, and there is no Linux arm64 build yet.

Full findings, including what was fixed and what is deliberately still open, are in [`docs/review/`](docs/review/).

## Requirements

- A coding CLI. `claude`, `codex`, `gemini`, `copilot` and `kimi` are all detected, but only `claude` can currently run the crew.
- Optional: git, an engine (Godot / Unity / Unreal), and for generated art `node`, a `python` with `pillow`, and the `codex` imagegen skill.

MIT licensed.
