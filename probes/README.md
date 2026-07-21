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

### Index vs reading files

Answers whether the symbol index ([02](../docs/design/02-context-engine.md),
[11](../docs/design/11-index-and-bootstrap.md)) actually spends fewer tokens than
letting a worker read the project, which the design asserted and had never
measured.

```bash
cargo build --release -p studiod
bash probes/index-tokens.sh              # 63-file Godot fixture
SCRIPTS=10 bash probes/index-tokens.sh   # quicker, weaker signal
```

Two workers, same fixture, same question, same charter, same model. The question
needs three facts spanning two scripts and a scene file: a method signature, the
file and line defining it, and the scene node mounting that script. One arm is
given `symbol_lookup` and **no file access**; the other is given `Read,Grep,Glob`
and **no index**. Both spawn the way the daemon spawns workers, with the brief on
stdin.

**Verdict: the index route costs roughly 2.3-3.4× fewer input tokens.** Both arms
answered correctly on every run.

| run | index route | file route | token ratio | cost ratio |
|---|---|---|---|---|
| 1 | 5299 | 12360 | 2.33× | 4.64× |
| 2 | 3608 | 12360 | 3.43× | 1.58× |
| 3 | 5299 | 12360 | 2.33× | 2.21× |

Billed input tokens. The index arm settles it in 2 `symbol_lookup` calls and 3
turns; the file arm takes 4 tool calls (`Grep`, `Grep`, `Glob`, `Read`) and 5
turns, and was reproducible to the token at 12360 every run.

**Do not quote a single cost figure.** Cost moved 4.64×, 1.58× and 2.21× across
those three runs, because it depends on how much of each arm's input arrives as a
0.1× cache read rather than a 2.0× cache write. The token ratio is the stable
measurement; the dollar ratio is mostly a statement about cache state.

Note also that this is a *retrieval* task, the index's best case. A task that
genuinely needs a whole file body will read one either way.

The script keeps the same nested-session guard as the M1 probes. That guard may
now be stale: these three runs were taken with `PROBE_FORCE=1` **from inside a
Claude Code session** on CLI 2.1.216, and both arms authenticated and completed
normally. The guard stays because the M1 failure was real when it was written and
a stale refusal costs nothing, while a silently unauthenticated run produces
confident garbage. Re-test before removing it.

## What running the probes caught

The token probe was written to confirm a savings claim and instead found a
correctness bug. Both arms were asked for the line number of a definition; the
index answered 11 and the file-reading arm answered 12. The file arm was right.
`tree-sitter` reports 0-based rows and the extractors were storing them raw, so
every line the index had ever reported was one short. Nothing in 419 tests
caught it, because every test asserted against the same off-by-one convention
that produced it. The fix is one helper and four tests that resolve a reported
line back against the real file text.

A probe that only confirms what you believed is a weaker probe than one that
disagrees with a second source.
