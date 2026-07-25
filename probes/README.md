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

## Floor probes

Answer where the studio floor's time goes, and what a low-spec machine is being
asked to draw. They run under Node with no browser and no GPU: `three.js` is
imported from `crates/studio-server/web/vendor`, the DOM and the 2D canvas are
stubbed, and `renderer.render` never happens. So they measure **the CPU inside
`animate()`, the shape of the scene graph, and allocation** — never a real frame.

```bash
node --expose-gc probes/floor-cost.mjs             # default tier
LOW_SPEC=1 node --expose-gc probes/floor-cost.mjs  # the low-spec tier
REV=<sha> node --expose-gc probes/floor-cost.mjs   # the same harness against an older revision
node probes/floor-smoke.mjs                        # does the floor still hold together
node probes/floor-latency.mjs                      # endpoint latency against a running daemon
```

`FRAMES` and `WARMUP` set the sample size; 3000 and 600 are enough for the 95th
percentile to settle, 900 is not. The layout comes from `probes/out/floor.json`,
which the probe fetches from `FLOOR_URL` (default `http://127.0.0.1:7878`) the
first time and caches; `probes/out/` is gitignored, so a daemon has to be up once.

`REV=<sha>` is the before-and-after switch: it extracts `web/*.js` at that commit
into a temp dir and runs the identical harness against it, which is the only way
to compare without trusting two different measurements.

`floor-cost.mjs` **reimplements the body of `floor.html`'s `animate()`** — the
same calls in the same order — because that loop lives inside an inline
`<script type="module">` and cannot be imported. Keep them in step or the numbers
stop meaning anything. `floor-smoke.mjs` is the guard: it extracts that inline
module, parses it, and checks every name it imports is actually exported.

### What they measured

Default tier, 3000 frames, same harness both sides, `2488330` against the work
that followed it:

| | before | after |
|---|---|---|
| whole `animate()` body, mean | 0.193ms | **0.108ms** |
| whole `animate()` body, p95 | 0.239ms | **0.132ms** |
| `scene.updateMatrixWorld`, mean | 0.149ms | **0.065ms** |
| objects walked per frame | 1106 | **510** |
| distinct materials | 253 | **135** |
| distinct geometries | 284 | **178** |
| canvas textures | 41 | **31** |
| desk-screen surfaces | 18 | **8** |
| `fillRect` per frame | 7.8 | **2.9** |
| `fillText` per frame | 4.2 | **0.5** |
| office build, first paint | ~85ms | **~59ms** |

The low-spec tier lands at **0.074ms mean / 0.108ms p95**, with 0 point and 0
spot lights instead of 22 and 13, shadows off, and pixel ratio 1.

What the numbers do **not** cover: draw calls, fill rate, shadow-map cost,
texture upload cost, and shader compilation. The scene census counts **604
renderable meshes, 472098 triangles, 361 shadow casters and 450816 triangles per
shadow pass** so the shape of the GPU problem is on the record, but no frame has
been timed on a GPU. See R15 in [13](../docs/design/13-risks.md).

### Hardware acceleration

`gpu.acceleration` (default **true**) asks the browser for
`powerPreference: "high-performance"` and, on a hybrid-graphics laptop, that is
what decides whether the floor gets the discrete GPU or the integrated one. It
also asks with `failIfMajorPerformanceCaveat: true` first, because a refusal is a
definitive answer at context-creation time that the browser would render in
software, which beats inferring it from frame times a few seconds later.

**Whether it makes a frame faster is not measured, and cannot be measured here.**
There is no GPU, no display, and no connected browser in this environment, and the
frame harness never rasterises. `floor-smoke.mjs` verifies the parts that *are*
checkable without one:

| what | how |
|---|---|
| three.js really forwards the flag | a canvas stub records the attributes `canvas.getContext` is called with, so this is observed rather than read off the source |
| acceleration on asks high-performance first, with the caveat flag | `contextAttempts` inspected directly |
| a refused context retries without the flag instead of failing | a fake renderer that throws three.js's exact "with your selected attributes" error |
| a refusal is reported as a software fallback | `hardwareHints().software` |
| a software fallback offers low spec at once, not after 240 frames | `shouldOfferLowSpec(hints)` |
| off asks for low power, in exactly one attempt | `contextAttempts(false)` |
| flipping the setting asks for a reload, and stops asking once rebuilt | `gpuNeedsReload()` |

To settle the speed question in about a minute on a real machine: open the floor
and read the line under the help text in the sidebar. It names the context that
was granted, the device it landed on, and the live 95th-percentile frame time.
Toggle the setting there, reload, and compare the two numbers.

### Endpoint latency

`floor-latency.mjs` times each floor endpoint 40 times against a running daemon
and reports p50/p95/max. It only issues GETs, so it is safe against a live
studio. `RUN=<run id>` adds the snapshot, resume and websocket-reconnect paths.

Against a read-only `studiod floor` over a **50000-event run generated through
the real write path** (`SEED_DB=<path> SEED_EVENTS=50000 cargo test -p studio-store
--release -- --ignored --nocapture a_long_run_can_be_written`):

| endpoint | p50 | bytes |
|---|---|---|
| `/` floor document | 0.48ms | 68788 |
| `/floor` layout | 0.29ms | 4817 |
| `/roles` | 0.27ms | 1436 |
| `/projects` | 0.27ms | 376 |
| `/settings` | 0.25ms | 2 |
| `/workflows` | 0.45ms | 474 |
| `/scene.js` | 0.34ms | 40412 |
| `/vendor/three.module.js` | 2.40ms | 1272972 |
| **`/games`** | **82.37ms** | 614 |

The resume paths, before and after the call sites were bounded:

| endpoint | before | after | bytes |
|---|---|---|---|
| `/runs/:run/snapshot` | 151.8ms | 146.1ms | 7667 |
| `/runs/:run/events?since_seq=0` | 152.1ms | 145.0ms | 7685 |
| `/runs/:run/events?since_seq=head` | 139.9ms | **0.26ms** | 64 |
| websocket reconnect at `since_seq=49900`, first frame | 139.4ms | **1.5ms** | 100 frames |

The two snapshot rows are meant to be flat: a snapshot legitimately reads the
whole run, and only its head lookup got cheaper. The other two are the fix.

1369 KiB crosses the wire before the floor can build, 1273 KiB of it `three.js`,
which is already sent with `max-age=86400`.

`/games` remains 170x the next slowest endpoint because it scans the filesystem
on the request path; it is owned elsewhere.

### Store scale

```bash
cargo test -p studio-store --release -- --ignored --nocapture
cargo test -p studio-events --release -- --ignored --nocapture
```

`events` is keyed `PRIMARY KEY (run, seq)`, so the SQL was never the problem —
asking for the whole run was.

| events in the run | whole log | tail of 100 | head only |
|---|---|---|---|
| 1000 | 11.87ms | 0.289ms | 0.076ms |
| 10000 | 32.49ms | 0.182ms | 0.051ms |
| 50000 | 124.08ms | 0.181ms | 0.051ms |

Twenty reconnects against that 50000-event run cost **2622.3ms** read whole-log
each time and **0.5ms** bounded. Pooling the read connections and caching the
prepared statements is most of the small numbers: the tail read was 0.925ms and
the head 0.660ms when every call opened its own SQLite handle.

The coalescer was checked on the same axis and is **linear**, about 0.4us per
event, 200000 events compacted in 78ms. Replacing its per-push `String` clone
with the entry API made no difference above run-to-run noise, so it was not kept.

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
