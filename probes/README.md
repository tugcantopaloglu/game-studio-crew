# M1 CLI probes

Settles the CLI behaviors the design depends on but could not confirm from
documentation. Run these before writing daemon code: two of the three
verdicts change the architecture.

## Run

```bash
bash probes/run-probes.sh
```

Defaults to `opus`. Override with `PROBE_MODEL=fable bash probes/run-probes.sh`.

Must be run from a normal terminal, not from inside a Claude Code session.
A nested `claude` spawned from within a session does not inherit
credentials and fails immediately with `Not logged in`.

## What each probe settles

| Probe | Question | If it fails |
|---|---|---|
| A | Does `stream-json` carry usage before the final `result`? | In-flight budget enforcement degrades to EMA estimation settling at `result` ([06](../docs/design/06-budget-governance.md)) |
| B | Does `--mcp-config` still attach under `--bare`? | Capsule submission falls back to a watched outbox directory ([00](../docs/design/00-overview.md)) |
| C | Does an identical frozen prefix hit cache across separate subprocesses? | The entire token thesis fails and the caching design needs rework ([02](../docs/design/02-context-engine.md)) |

Probe C is the load-bearing one. `prefix.txt` is generated at 5033 tokens,
above the 4096 Opus minimum cacheable prefix, because a shorter prefix
caches silently on Fable but not on Opus.

## Confirmed already

Established while building this harness:

* `-p` with `--output-format stream-json` requires `--verbose` or the CLI errors out.
* Stdin must be redirected explicitly (`< /dev/null`), otherwise the CLI waits 3s for stdin and warns.
* The final `result` event carries `usage`, `total_cost_usd`, `modelUsage`, `session_id`, and `terminal_reason`.
