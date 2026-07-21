# Probes

Settles behaviors the design depended on but could not confirm from
documentation. Two groups: the **M1 CLI probes**, which settled how the `claude`
CLI actually behaves, and later **cost probes**, which answer whether a piece of
machinery is worth building.

## M1 CLI probes

**All three verdicts are in**, and one of them overturned a founding assumption.
See [ADR 0004](../docs/design/adr/0004-explicit-context-control-not-bare.md).

### Run

```bash
bash probes/run-probes.sh
```

Defaults to `opus`. Override with `PROBE_MODEL=fable bash probes/run-probes.sh`.

Must be run from a normal terminal, not from inside a Claude Code session.
On Windows use Git Bash explicitly, since PowerShell resolves `bash` to WSL:

```powershell
& "C:\Program Files\Git\bin\bash.exe" probes/run-probes.sh
```

### Results

| Probe | Question | Verdict |
|---|---|---|
| A | Does `stream-json` carry usage before the final `result`? | **YES.** `stream_event`/`message_start` carries a full `usage` block; 4 pre-`result` events carried usage in a short turn. No EMA fallback needed. |
| B | Does `--mcp-config` attach? | **YES**, with the ADR 0004 flag set: `status: "connected"`, tool advertised, invoked, value returned. No outbox fallback needed. |
| C | Does an identical frozen prefix hit cache across separate subprocesses? | **YES.** 8867 written cold ($0.0888), 8867 read warm ($0.0051). **17.4×.** |

### What the probes overturned

`--bare` **cannot be used.** It fails `Not logged in` in 222 ms against valid
subscription credentials, because it reads auth strictly from
`ANTHROPIC_API_KEY` or `apiKeyHelper`. The design named it "the primary token
lever" across four documents and an ADR, having verified it as *documented*
behavior without ever executing it.

`--safe-mode` was evaluated as the replacement and rejected: auth works, but
MCP servers are disabled unconditionally and neither `--mcp-config` nor
`--strict-mcp-config` overrides that.

The working configuration strips context explicitly and keeps OAuth:

```
claude -p --setting-sources "" --system-prompt-file <charter>
  --tools "<role allowlist>" --allowedTools ...
  --mcp-config <cfg> --strict-mcp-config
  --permission-mode dontAsk
  --output-format stream-json --include-partial-messages --verbose
```

## Token measurements (Opus 4.8, single `say ok` turn)

| Configuration | input tokens | cost |
|---|---|---|
| default, nothing stripped | 22572 | $0.2258 |
| `--safe-mode` | 21329 | $0.0517 |
| `--safe-mode --system-prompt` | 19510 | $0.1952 |
| `--safe-mode --system-prompt --tools ""` | **184** | **$0.0010** |

The dominant term is **built-in tool schemas**, not `CLAUDE.md` or ambient
context: replacing the system prompt leaves 19.5k, emptying the tool list
leaves 184. `--tools` is the real token dial, and because it is part of the
cache key, a role's allowlist fragments the cache exactly as its charter does.

## Cache facts corrected

* TTL is **1 hour** (`cache_creation.ephemeral_1h`), not 5 minutes.
* Write premium is **2.0× base**, measured exactly, not 1.25×.
* Minimum cacheable prefix (Opus 4096 / Fable 2048) is **documented but never
  probed**. Consistent with observation (184 tokens cached nothing, 8867 did),
  but the threshold was not isolated.

## Confirmed while building the harness

* `-p` with `--output-format stream-json` requires `--verbose` or the CLI errors out.
* Stdin must be redirected explicitly (`< /dev/null`), otherwise the CLI waits 3s and warns.
* The final `result` carries `usage`, `total_cost_usd`, `modelUsage`, `session_id`, `terminal_reason`.
* On Windows, paths written into `mcp.json` must be Windows-form (`cygpath -m`);
  a Git Bash `/c/...` path reaches a native `node` that cannot resolve it.

## Cost probes

### Index scan cost

Answers whether the `notify` filesystem watcher specified in
[11](../docs/design/11-index-and-bootstrap.md) is worth building, given that the
studio already rescans the project around every command.

```bash
cargo build --release -p studiod
bash probes/index-scan.sh                 # 40 modules x 50 units = 4001 files
MODULES=10 UNITS=10 bash probes/index-scan.sh   # quicker, smaller
```

**Verdict: the watcher is declined.** On a 4001-file synthetic Godot project,
release build:

| | elapsed |
|---|---|
| cold, nothing indexed | **2.50s**, once |
| warm, not one byte changed | **0.24s** |
| warm, one script edited | **0.24s** |

About 60µs per file, so even a 40k-file project lands near 2.4s. Each command
spawns `claude` workers that run for seconds to minutes, which puts the refresh
under one percent of the command it hangs off. A watcher would need a thread,
debouncing, tolerance for editors that write via temp-file-and-rename, and a
reconciling scan anyway because it can drop events under load — a second
mechanism bought with a measured sub-one-percent saving.

The number is what makes this a decision rather than a deferral. Re-run the
probe if a project ever makes the refresh visible.
