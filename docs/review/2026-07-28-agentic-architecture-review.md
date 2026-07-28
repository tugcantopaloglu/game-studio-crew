# Agentic Architecture & Backend Review

> **Method:** two models (Opus 5 and Fable 5) reviewed the backend independently with an identical brief and no knowledge of each other, then cross-examined each other's findings in a second pass with instructions to confirm, refute, or retract against the source. Frontend, desktop shell and installer were out of scope.
>
> **Date:** 2026-07-28 · **Commit:** `ccba7ff` · **Scope:** ~33k lines of Rust across 15 crates
>
> **Caveat that applies to everything below:** both passes were static. Nothing was executed, no test suite was run. Findings are graded by whether both reviewers independently reached them and whether the mechanism survived adversarial re-reading — not by observed behaviour. Section 7 lists what only running the code can settle.

---

## 0. Status

Fixed since the review, each with a test that fails without the change:

| § | Finding | What changed |
|---|---|---|
| 2.2 | Cold-prefix stampede | Waves group by prefix hash; one worker warms each cold prefix before its siblings run. `warmed` is keyed on the real hash with a 1-hour TTL, not on the role name |
| 2.3 | Ladder inert, ratchet | The admitted rung now reaches the spawn (effort downshift, summarizer routing). The rung follows budget pressure instead of advancing once per admission, so it can no longer walk itself to a hard stop |
| 2.4 | Failed workers were free | Usage accumulates per turn instead of overwriting; cost falls back to the dollar mirror when no terminal result arrived; the `Err` path charges the run |
| 3.1 | Stale stop flag | `run_command` clears it, so a stop no longer kills every later task and meeting |
| 3.2 | Approval wedge | `recv_timeout` loop that polls the stop flag and re-announces the pending gate |
| 3.3 | Forgeable gate evidence | Helpers are restored before **each** gate; a rewritten check fails the gate by name instead of passing silently |
| 3.4 | Gate on the wrong node | Gates attach to the deepest leaf — a node in the final wave — not to whatever was declared last |
| 3.5 | Unrunnable engine scopes | `{preset}`, `{platform}`, `{ue_root}`, `{uproject}`, `{target}`, `{suite}` are derived; an engine root in `UE_ROOT` resolves; a scope whose script is missing names it |
| 3.6 | `verify()` outliving its timeout | Uses `ProcessGroup`, kills the tree, and abandons readers after a grace period instead of joining them forever |
| 3.7 | False-pass parsers | Empty UE suite, truncated NUnit3, and NUnit `Inconclusive` are no longer passes; a silent studio helper is Inconclusive; the infrastructure match is per-line and ignores file names |
| 3.10 | Stall watchdog | The stall clock pauses while a tool call is open, and a terminal result outranks the watchdog |
| 3.11 | Resume durability | Staged write, read-back check, then rename; progress is recorded after gates and after the human verdict |
| 4 | DNS rebinding | `Host` allowlist on every request |
| 2.1 | Capsule event | The fabricated `CapsuleSubmitted` no longer fires for crashed or killed workers |

**Not done, and deliberately so:**
- **Wiring the MCP capsule channel (§2.1).** The `busy_timeout` prerequisite is now in place, but attaching MCP changes every prefix hash and needs a run against the real CLI to confirm. It is the largest remaining item.
- **A session token on the control plane (§4).** Every option requires changing the floor's JS, which was out of scope for this pass. The rebinding half is closed; the local-process half is not. `GET /ws` still broadcasts `approval_id` unscoped, so the escalation chain in §4 remains open.
- **Per-node worktrees (§3.9).** The two reviewers disagreed on the fix, and the cheaper alternative they converged on (post-wave overlap detection) depends on capsules being wired first.
- **Unix signal handling (§3.8).** Windows is the shipping platform; this needs a handler plus a `Drop` on `Worker`.

---

## 1. The headline

The primitives are genuinely well-built. The seams are where this fails.

Charter freezing, process supervision on Windows, the store's single-writer actor, plan validation and the `verify()` inconclusive-first contract are careful, adversarially tested, and in several places better than the documents describing them. Both reviewers said so independently and without prompting.

But **the system the docs describe and the system that runs are two different systems.** The agentic layer this project is named for — capsules, the MCP trust boundary, delegation, escalation, the summarization ladder, the R0–R4 trust model — is fully implemented as libraries with near-complete unit tests, and never wired into the running studio. A single line (`m4.rs:486`, `mcp_config: None`) disconnects five crates' worth of design. The daemon then fabricates a `CapsuleSubmitted` event from raw stdout text, so the studio floor still looks correct while nothing behind it validated, rendered, capped or stored a capsule.

What actually runs is a linear plan → spawn → commit → verify pipeline whose only inter-agent channel is the shared filesystem, whose budget ladder emits events but changes nothing about the spawns, and whose frozen charter instructs workers to use tools that are never attached.

**Both reviewers, separately, identified the same single biggest risk:** the economic thesis and the agentic thesis fail in the same place. The parallel wave scheduler fires N same-prefix workers simultaneously — the cold-write stampede `docs/design/02` explicitly forbids — so the 17.4× cache saving the whole design is built around is discarded on exactly the workload shape the planner is instructed to produce.

---

## 2. Convergent findings — both reviewers, independently

These reached the same conclusion from two separate readings. Treat them as settled.

### 2.1 The capsule/MCP channel is not wired, and the daemon manufactures evidence to cover it
**Critical** · `studiod/src/m4.rs:486`, `m4.rs:602-612`

`run_worker_inner` is the only spawn path used by `studiod studio` — tasks, meetings, builds, workflow nodes, repair. It builds `WorkerSpec { mcp_config: None, .. }`, and `allowed_tools` comes from `Role::tools()`, which never contains an `mcp__studio__*` name (`studio-agents/src/lib.rs:91-101`). `mcp_config: Some(..)` appears exactly once in the repo — the M2 demo at `main.rs:496`.

Verified consequences:
- `capsule_submit`, `decision_search`, `symbol_lookup`, `escalate`, `request_meeting` are unreachable by any production worker.
- `Store::insert_capsule` is never called; the `capsules` table stays empty; `do_not_revisit` is never captured or propagated.
- `studio_context::ladder` has zero non-test callers.
- Seven declared, doc-tested event types are never emitted: `SummaryCreated`, `TaskDelegated`, `TaskReturned`, `ConsultRequested`, `ConsultAnswered`, `Escalated`, `InconclusiveFlagged`.
- In their place, `m4.rs:602` emits a synthetic `CapsuleSubmitted` with `"summary": report.state.text.trim()` and `"truncated": false` — **and it fires before the outcome check at `m4.rs:634`**, so crashed, killed and timed-out workers also show a submitted capsule on the floor.
- `node_brief` hands dependents `"Upstream capsules: cap_t0, cap_t1"` (`wf.rs:90-94`) — dangling identifiers, in the dependent's *paid* volatile layer, that the worker cannot dereference and may hallucinate around.

Meanwhile the frozen L0 charter (`charters.rs:6-19`) tells every worker "Capsules are the only inter-agent channel. Emit exactly one capsule… Escalate to your declared parent role" — instructions with no attached mechanism, and those bytes ride inside every cached prefix and are paid for on every warm read.

*Failure scenario:* an engineer hits a dead end, says so in prose, the daemon records the prose as a capsule and drops it. The next wave re-derives the same dead end at full price. `do_not_revisit` never fires once.

**Two warnings on the fix, both raised during cross-examination:**
- The cheap interim ("inline `Metered.text` into dependent briefs") reintroduces exactly what the capsule schema exists to bound. `Capsule` has a 512-token summary cap and a 4k render cap with a documented truncation order (`capsule.rs:135-141`, `214-268`); raw `Metered.text` has none. If taken as an interim, apply `truncate_tokens`.
- **Wiring MCP puts a second writer process behind every worker — and there is no `busy_timeout` anywhere in the workspace** (grep confirms zero hits; `studio-store/src/schema.rs:6-8` and `studio-index/src/schema.rs:6-7` set only `journal_mode`, `synchronous`, `foreign_keys`). SQLite's default busy timeout is 0, so a second writer gets `SQLITE_BUSY` immediately with no retry. Fixing the capsule loop converts a latent bug into a live one on every spawn. Ship them together.

### 2.2 Parallel waves cold-write the same prefix N times
**High** · `studio-workflow/src/exec.rs:274-291`, `wf.rs:334`, `studio.rs:113-117`

`execute_parallel` spawns every node in a chunk at once inside `thread::scope`. `Host.warmed` — the only warmth tracker — is inserted *after* `enter` returns (`wf.rs:334`). The director's brief explicitly instructs "parallelize aggressively… the same role may appear more than once." So four `gameplay_engineer` nodes in one wave freeze byte-identical charters and all four pay the measured 2.0× cache-write premium simultaneously.

From the repo's own pinned constants (`studio-budget/src/lib.rs:361-367`): 4 × $0.0887 = $0.355 against $0.0887 + 3 × $0.0044 = $0.102. `docs/design/02` names this exact case as the thing the scheduler must not do.

Two secondary defects compound it: `prefix_is_warm` keys on `node.role` (`wf.rs:281`) rather than `prefix_hash`, so it cannot see the acting/non-acting charter split or the 1-hour TTL; and `prefix_tokens_for(r, false)` (`wf.rs:267`) always projects the non-acting charter even for acting nodes.

*Severity note:* graded **High** rather than Critical after cross-examination — it applies to each prefix's first wave, and steady-state runs are warm. First-run economics only, but that is where most user impressions form.

### 2.3 The degradation ladder is recorded and never applied
**High** · `exec.rs:118`, `exec.rs:242`, `wf.rs:285-292`

`Admission::Degrade { step }` is handled by `report.degradations.push(step)` and nothing else. The node then spawns unchanged — same seat, same model, same effort. `Effort::downshift` and `model_for_step` have **zero non-test callers** in the workspace. Force-summarize does not exist in any form. Only `HardStop` has teeth.

Two structural problems underneath it:
- `Enforcer::new(ceiling_tokens, ceiling_tokens)` (`wf.rs:793`) collapses task and sprint scope into one number, so doc 06's two-scope model is nominal.
- `total_budget()` = nodes × 120 000 (`plan.rs:5`) against `billable_tokens` that discounts cache reads 10× makes the token ceiling effectively unreachable. The `$25` USD ceiling at `wf.rs:244-264` is the only real limiter — a bare `Refuse` with no ladder at all. This is the exact inversion of doc 06's "the USD number never gates anything."

**The ratchet (Fable's addition, confirmed):** `Enforcer::admit` (`studio-budget/src/lib.rs:239-252`) advances `applied` one rung *per admission* once projected `after >= WARN_AT` (75%), stepping `None → EffortDownshift → SummarizerDownshift → TrimL3 → ForceSummarize → HardStop`, and nothing ever de-escalates when pressure falls. Since the steps are never applied, nothing relieves the pressure, so ladder exhaustion is deterministic once armed. `WaveVerdict::Redo` (`exec.rs:360-371`) re-admits nodes, so three human send-backs burn three extra rungs.

*Corrections applied during cross-examination:* the admission that sets `HardStop` still returns `Degrade`, so that node runs — only the *next* is refused. And the worked "12-node run dies at 80%" example does **not** hold under shipped defaults; the ratchet requires a user-configured low `budget.tokens`. Real mechanism, conditional trigger.

### 2.4 Failed workers are free
**High** · `wf.rs:331-338`, `m4.rs:634`, `m4.rs:644`, `studio-core/src/stream.rs:249-251`

Two halves, one found by each reviewer, and together they are worse than either alone.

*Stream side:* `StreamState::apply` **overwrites** `latest_usage` on each `UsageDelta`. A worker killed at its 45-minute wall clock after 30 agentic turns records only the last message's usage block, with `cost_usd: 0.0` because the terminal `result` never arrived.

*Caller side:* `Host::enter` records spend **only on the `Ok` path**, and `run_worker_inner` bails on any non-`Completed` outcome and on `is_error`. So a worker that ran 45 minutes and was killed, or one that did real work and then hit the account limit, contributes **exactly zero** to `spent_usd` — which is the only ceiling that actually fires. `Host::repair` and `MeetingSpend` have the identical shape.

The ledger row *is* written (`m4.rs:538`, before the bail), so the database and the enforcer disagree, and the enforcer is the one holding the money. **The failure modes the budget most needs to see are precisely the ones it is blind to.**

### 2.5 The R0–R4 trust model is dead code, and the one human gate auto-passes
**Medium-High** · `studio-standards/` (no reverse dependencies), `wf.rs:798`

No crate depends on `studio-standards`; `assess()`, `Trust::gates()`, `RuleMode` and `enforced_rules()` have no callers outside their own tests. The only approval mechanism that exists, `GateKind::Approval`, is decided by `self.auto_approve` — hardcoded `true`.

Practical consequence: every acting worker runs at maximum trust with `Bash` + `--permission-mode dontAsk` and no review gate except engine verify, which §3.3 shows is forgeable.

Worth noting for whoever wires it: `FileChange` (`studio-standards/src/lib.rs:97-108`) carries `covered_by_passing_test`, `public_signature_changed` and `incoming_refs` as `#[serde(default)]` deserializable fields that `assess()` reads as fact — a self-report surface for exactly the claims that need external evidence. Two of the three are derivable from the index the daemon already builds.

### 2.6 Engine knowledge is in the wrong layer
**Medium (cost), High (coherence)** · `m4.rs:205-224`, `wf.rs:753-788`

`charter_for` always uses `L1_GENERIC_ENGINE`, which literally says "No engine profile is bound to this invocation." The real profile prose (`godot.toml [prose]`) is loaded only by `m3_proof`. Meanwhile a ~250-token engine hint rides in the **volatile** brief on every single node spawn — paid uncached, every time, forever — and it instructs the opposite of the charter above it: the charter says "never run engine commands," the hint says "run the engine binary from `GODOT_BIN`."

Both reviewers converged on the fix: freeze `EngineProfile.prose.profile` into L1, one prefix per engine per role, as doc 02 designed. Moves those tokens behind the 0.1× cache-read rate, removes the contradiction, and starts earning value from the already-written Unity/UE5 prose.

---

## 3. Findings raised by one reviewer and confirmed by the other

### 3.1 A stale stop flag silently kills every later task and meeting
**High** · `studio-server/src/lib.rs:220-223`, `m4.rs:520`, `wf.rs:818`
*Raised by Fable; confirmed by Opus, which called it "the highest consequence-per-line finding in either review."*

`interrupt({stop:true})` sets a shared `stopping` AtomicBool. Every worker is armed with it, and `Worker::drive` checks it as the **first statement of the drive loop** (`studio-core/src/lib.rs:191`), before any line is read. Grep across the whole workspace confirms the only reset is `wf.rs:818` — and it sits *before* `execute_parallel`, making it a run-**start** reset, not a run-end one. Nothing in `run_command`, `run_task`, `run_meeting` or the HTTP layer clears it.

So: stop a guided run, then ask a single question. The `claude` process really is launched and then killed within ~0–250 ms. Token cost is near zero; the cost is that every standalone Task and every Meeting participant returns `Outcome::Killed`, meetings print "X did not speak" for every seat and adjourn empty, and this persists until the user happens to start a Build — the only path back through `run_planned`. Silent, misattributed, persistent, and the natural user sequence hits it every time.

**Fix:** `em.state.nothing_is_being_stopped()` as the first line of `run_command` (`studio.rs:61`).

### 3.2 An unanswered approval wedges the entire daemon
**Medium-High** · `wf.rs:225`, `wf.rs:444`, `studio.rs:263`, `studio.rs:865-915`
*Raised by Fable; confirmed by Opus with one leg corrected.*

`spend_approved`, `after_wave` and `propose` all call `Receiver::recv()` with no timeout. The Sender lives in the approvals map and is removed only by `resolve_*`, so inaction can never produce `RecvError::Disconnected` — the error arms are unreachable by inaction. `serve_studio` is `for cmd in rx { run_command(..) }` on **one thread**, so the wedge is daemon-wide, and the stop interrupt cannot break it because it is only polled between waves.

*Corrected:* Fable cited the WebSocket dropping lagged clients as a trigger; the reconnect path replays backlog from `since_seq`, so a dropped socket re-delivers the approval request. The real trigger is simply a human closing the tab or walking away.

**Fix:** `recv_timeout` in a loop that also polls `stopping`.

### 3.3 The party being judged can write the judge
**High** · `wf.rs:727-731`, `studio-engine/src/lib.rs:928-953`, `parsers.rs:456-461`
*Raised by Fable; confirmed by Opus, which made the attack strictly easier.*

Gate helper scripts live **inside the tree acting workers own**: `addons/studio/studio_ci.gd` for Godot, `tools/studio_ci.mjs` and friends for web. All four acting tool classes carry `Edit`+`Write`; two carry `Bash`; every spawn runs `--permission-mode dontAsk` with cwd = project root and no path scoping.

`install_helpers` restores tampered helpers via `write_if_changed` — but it is called **exactly once per run**, before any node executes. A wave-1 worker owns every gate for the remainder of the run.

**Opus's sharpening matters for the fix:** Fable described the forge as requiring the worker to print `STUDIO_CI_DONE`. It doesn't. `parsers.rs:460-461` already returns `pass()` for `(exit 0, no parsed errors)`; `saw_helper_summary` is a redundant positive shortcut. **A helper rewritten to `process.exit(0)` silently passes.**

This is the sharpest trust-boundary hole either review found: the artifact that decides whether work is acceptable is writable by the party being judged, and the acceptance rule is "said nothing and exited zero."

**Note for the fix:** requiring `STUDIO_CI_DONE` does *not* close this — a tampered helper prints it happily. Closing it requires re-installing or hash-checking helpers immediately before **each** gate evaluation, and treating a content mismatch as a named Fail rather than a silent rewrite.

### 3.4 The verify gate can fire mid-run against a half-built project
**High** · `studio.rs:333-345`, `exec.rs:336-345`
*Raised by Opus; confirmed by Fable.*

```rust
let after = match (leaves.next(), leaves.next()) {
    (Some(only), None) => only.to_string(),
    (Some(_), Some(_)) => wf.nodes.last()?.id.clone(),
    _ => return None,
};
```

With ≥2 topological leaves the gate is pinned to the last node in the director's **declaration order**, with no check that it is a leaf or runs in the final wave. Gates fire at the end of whatever wave that node completed in.

Fable's addition: the single-leaf arm is sound (in a DAG with one leaf, every path terminates at it, so it necessarily runs last) — only the multi-leaf fallback is broken. And the planner is explicitly told to parallelize aggressively, which produces multi-leaf plans routinely.

Two consequences: a compile gate evaluates a half-written project and burns up to three acting repair workers (45-minute wall clocks each) on failures later waves would have fixed, then `RunOutcome::Escalated` halts the run at partial completion — *and* even in the benign ordering, all work in waves after the pinned node ships completely unverified.

### 3.5 Unity and UE5 cannot execute a single scope — and Godot's export and test scopes are broken too
**High** · `studio-verify/src/driver.rs:108,113-122`, `studio-engine/src/lib.rs:212-234`
*Raised by Opus, flagged by Opus as its own least-confident area; Fable scrutinized it first-hand and confirmed every particular.*

`ProfileDriver::substitutions` binds only `engine`, `project`, `out` plus `self.extra` — and `extra` is `Vec::new()` at construction and never populated outside tests. Every other placeholder therefore dies at `UnboundPlaceholder` → `Inconclusive`:

- `{ue_root}`, `{target}`, `{platform}`, `{uproject}`, `{suite}`, `{source}` — ue5.toml
- `{platform}` — unity.toml
- **`{preset}` — godot.toml:32**, so Godot's export scope is broken as well

Compounding:
- `find_binary` never reads `tooling.resolver`, so `"unity_hub"` and `"env"` are dead metadata.
- `usable()` requires `is_file()`, so `UE_ROOT` — a directory by convention — can never resolve.
- `unity.toml`'s `-executeMethod Studio.CI.Compile` names a C# helper that does not exist in the repo.
- Godot's test scopes invoke `addons/gut/gut_cmdln.gd`, which `install_helpers` never installs (doc 11:29 claims bootstrap does).

**Both reviewers revised their own framing on this.** Fable conceded its round-1 "Unity/UE5 profiles are written but unprobed" was materially weaker than reality. Opus supplied the refinement that keeps it honest: because `verify_gate` only ever attaches `Compile` and `Runtime`, the missing GUT does not break `POST /build` — it breaks the builtin TOML workflows, whose feature/bugfix gates use `test_fast`.

**As shipped, Godot's working scopes are `compile`, `import` and `runtime`. `test_fast`, `test_full` and `export` are permanently inconclusive, on every engine.**

### 3.6 `verify()` can hang forever after timing out
**High** · `studio-verify/src/driver.rs:33-50,63-74`
*Raised by Opus; confirmed by Fable, which withdrew its own praise of this code path.*

The reader threads are correctly spawned before the wait loop, so there is no pipe-buffer deadlock in the normal case. But the timeout path is `child.kill(); child.wait();` — the immediate child only. There is no Job Object and no process-group kill here, unlike `studio_core::ProcessGroup`, which exists precisely for this and lives forty lines away in the same workspace.

Grandchildren (UBT → cl.exe under `Build.bat`, Unity's Roslyn, a backgrounded `node` server) inherit the stdout write handle, so the unconditional `out_handle.join()` blocks in `read_to_end` until every writer closes. **The 900-second bound governs the wait loop, not the function.** `verify()` can block the entire sequential command loop indefinitely after "timing out." The existing test passes only because `node -e setInterval(...)` has no grandchildren.

`docs/design/13` R7 names orphaned lock-holding editors as exactly what Job Objects exist to prevent — the protection is in `studio-core` and absent in `studio-verify`.

### 3.7 Four ways `verify()` passes on false evidence
**High as a cluster** · `studio-verify/src/parsers.rs`
*Raised by Opus; confirmed by Fable with two calibrations.*

- **Exit-0 with a silent log is a Pass** (`parsers.rs:461`). There is no "a helper was invoked and never spoke" branch, and the driver doesn't tell `scan_log` whether the command was a helper. Unity's compile/import/export redirect all output to `-logFile {out}/compile.log`, which nothing in the repo ever reads — so the "structured log scan" sees an empty string and the verdict is exit code alone.
- **`{"tests":[]}` is a Pass** in the UE parser (`parsers.rs:319`): the guard is `if !recognised_state && !tests.is_empty()`, and an empty array skips it. JUnit and NUnit3 both have the `cases == 0` guard UE lacks.
- **NUnit3 has no truncation guard** while `parse_junit` tracks `depth` and rejects unclosed documents. The existence of junit's manual counter is the repo's own evidence that `quick_xml` returns a clean `Eof` on unclosed documents. *Calibrated to Medium alone* — narrow trigger window, since the driver deletes stale reports and kills → Inconclusive before parsing.
- **NUnit `result="Inconclusive"` counts toward Pass.** *Calibrated:* Opus framed skipped-tests-as-pass as a defect; Fable correctly noted that is standard CI semantics. The sharp half is NUnit's `Inconclusive` state — which literally means "did not decide" — being read as success.

**The inverted test.** `parsers.rs:744-754`: `fn a_helper_run_that_never_printed_its_summary_is_not_a_pass()` asserts `Verdict::Pass`. The function name states the invariant doc 08 wants; the assertion enforces its negation. *Opus partially retracted its own framing here* — the assertion message ("a plain exit-zero with no helper output still falls back to the generic rule") shows the author chose the behaviour deliberately, so this is a stale name over a rewritten test, not a bug accidentally blessed. The underlying defect stands, and §3.3 supplies the attacker.

**The mirror-image defect.** `looks_like_infrastructure` (`studio-verify/src/lib.rs:95-114`) is an unanchored, lowercased substring match on `"license"`, `"vulkan"`, `"no gpu"` over the *entire* log — and `scan_log` runs it **first**, before any failure parsing. A real compile error in a file named `licensing.gd`, or any engine banner containing "Vulkan", converts a repairable Fail into `RoutedToInfra` — a terminal halt, with no infra queue to receive it and `InconclusiveFlagged` never emitted.

### 3.8 On Unix, killing the daemon orphans the entire worker tree
**Medium (platform-weighted)** · `studio-core/src/proc.rs:29-33`, `crash.rs:24-53`
*Raised by Opus; confirmed by Fable, which made it worse.*

`ProcessGroup::prepare` calls `cmd.process_group(0)` on Unix, detaching workers from the daemon's foreground group, so a terminal SIGINT never reaches them. Grep confirms no SIGINT/SIGTERM/ctrlc handler anywhere in the workspace — `crash.rs` installs a panic hook only. Windows is covered by `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, whose handle closes when the daemon dies; Unix has no equivalent and `Worker` has no `Drop`.

**Fable's escalation:** the 45-minute wall clock is enforced by the daemon's own `drive()` thread — which is gone. So an orphaned worker doesn't run to its wall clock, it runs **forever**, still editing the project, with nobody recording, committing or metering it.

Fable's round-1 praise of process supervision was accurate for Windows (the primary platform) but was Windows-shaped without saying so.

### 3.9 Four parallel workers share one working tree with only a prose promise
**Medium-High** · `wf.rs:125-129`, `m4.rs:504-509`, `git.rs:95`
*Raised by Opus; confirmed by Fable, which disputed the fix.*

Concurrency control between wave workers is the sentence *"Other crew members may be editing different files at the same time, so touch only the files your own brief needs."* Nothing enforces disjointness. `commit_wave` then runs `git add -A` and sweeps whatever is on disk into one commit — **a lost update between two concurrent workers is committed as if it were intended work.**

*Disputed fix:* Opus proposed one `git worktree` per parallel node. Fable pushed back — heavy for engine projects with import caches, `.godot/` and binary assets — and proposed post-wave conflict detection instead: diff each worker's claimed artifact set (which you'd have if capsules were wired — the findings compose) or snapshot `git status` between spawns, and fail the wave loudly on overlap. **Reasonable disagreement; the cheaper option is the better first step.**

### 3.10 The stall watchdog measures stream lines, not worker liveness
**Medium** · `studio-core/src/lib.rs:200-203`
*Raised by Opus; confirmed by Fable.*

`last_line` resets only on an NDJSON line from the CLI. During a single long tool call the CLI emits `content_block_start` and then nothing until the tool returns. An acting worker's stall budget is 300 s — and the daemon's *own* engine hint instructs QA and art roles to run `node tools/screenshot.mjs`, `node tools/model_export.mjs`, and the Godot binary with `--write-movie`. Any of those exceeding five minutes reads as `Outcome::Stalled`, the tree is killed, and the node fails.

Related, from Fable: `Outcome` classification checks `stalled` **before** `saw_result` (`studio-core/src/lib.rs:247-259`), so a worker that genuinely finished but whose stdout is held open by a backgrounded grandchild is reported `Stalled` and fails its node.

### 3.11 Non-atomic resume file, and progress recorded before verification
**Medium** · `studio-server/src/resume.rs:60-65`, `exec.rs:332-334`
*Raised by Opus; confirmed and sharpened by Fable.*

`resume.rs` uses a bare `std::fs::write` with no staging + rename, while `write_charter_into` gets this right for a *less* important file. `read()` swallows any parse error into `None` — and the test `a_corrupt_record_is_ignored_rather_than_stopping_the_studio` proves the loss is silent. A crash during `record_progress` discards the whole resumable run.

Ordering: `wave_done` → `record_progress` is called **before** gates and **before** `after_wave`. Fable's sharpening: a node whose verify gate then *fails* is already recorded as done — and on resume, gates only fire after a node completes in-run, so a resumed run never re-evaluates that node's gate. **Failed-verification work resumes as verified-by-omission.**

---

## 4. The control plane — reconciled

The two reviewers initially disagreed here: Fable called the origin guard "above the bar," Opus called the control plane unauthenticated. Cross-examination resolved it into three distinct threat models, and Fable acknowledged its round-1 phrasing contradicted its own finding.

| Threat | Status | Evidence |
|---|---|---|
| Browser CSRF on writes | **Defended** | `guard_origin` (`lib.rs:307-312`) requires state-changing methods with an `Origin` to match `origin_is_local`, which correctly rejects `http://127.0.0.1.evil.test` (`lib.rs:1445`). POST always carries `Origin`, so rebinding cannot write. |
| Browser DNS-rebinding on reads | **Undefended** | `lib.rs:320` — a GET with absent `Origin` is unconditionally allowed, and browsers omit `Origin` on same-origin GETs. `Sec-Fetch-Site` is checked only in the write arm, and there is no `Host` check anywhere — which is the actual rebinding defence. Exposes `/fs/browse` (whole-machine directory enumeration, `fsapi.rs:66-101`), `/settings`, `/git/tree`, `/runs/*/snapshot`. |
| Local non-browser process on writes | **Undefended — and this is the one that matters** | No token on any route. The repo's own test `a_plain_client_with_no_browser_headers_is_still_served` makes it deliberate. |

The third row matters most because **this system's own workers are local processes with shell access.**

### The privilege escalation chain

Closed during cross-examination; neither reviewer had it alone:

1. `GET /ws` with no `run` query parameter is an **unfiltered live feed of every event of every run** — `lib.rs:1101` is `if q.run.as_deref().is_some_and(|r| r != e.run) { continue; }`, and with `q.run == None` that predicate is false, so nothing is filtered.
2. The upgrade is a GET, so `guard_origin` waves it through for any non-browser client.
3. That feed carries `approval_id` (`wf.rs:428`) and `plan_id` (`studio.rs:229`).
4. `POST /run/step {"approval_id": …, "verdict": "approve"}` (`runplan.rs:140-161`) then resolves the blocked `after_wave`.

**So a `Bash`-carrying `qa_engineer` can subscribe to `/ws`, read the approval request for the wave it is in, and approve its own work past the human gate** — while separately being able to rewrite the CI helper the verify gate executes (§3.3). The two human-authority mechanisms in the system are both reachable from inside the sandbox the system does not have.

Same surface exposes `POST /settings` (repoints the engine binary via `apply_engine_paths`) → `POST /play`; `POST /revert` (`git reset --hard` + `git clean -fd`); `/git/rollback`, `/git/push`, `/git/remote`; `/assets/generate`.

*Fable's scoping, which is fair:* for `Bash`-holding workers (Qa, ArtAudio) this is sabotage-of-the-supervisor rather than new code execution — they already have a shell. For Engineer and Designer roles (Read/Grep/Glob/Edit/Write only, no exec, no network tool) it is not reachable.

**Latent, not live:** `launcher_for` routes `.cmd`/`.bat` shims through `cmd.exe /c` (`launcher.rs:20-27`), which re-parses the argument vector — and `BriefDelivery::PromptArgument` puts the full model-generated brief into argv. A brief containing `&` would split the command line. Unreachable today because `Provider::blockers` refuses every non-Claude provider and `claude` is asserted not to resolve to a `.cmd` — but this becomes command injection, with a trigger as ordinary as an ampersand in a task brief, the moment any provider is unblocked.

---

## 5. What is genuinely well built

Both reviewers volunteered these independently. This is not padding — it calibrates everything above.

- **`studio-context::freeze` is the best-engineered code in the repo.** Line-ending normalization, `{{` rejection, empty-layer rejection, sorted tool list hashed with explicit separators, the model entering the hash as its **CLI alias bytes rather than its enum discriminant** (with a test explaining exactly why), padding above the documented minimum with meaningful prose rather than whitespace, and four golden `prefix_hash` values pinned in tests so that adding a model variant cannot silently cold-start every warm prefix in the wild. The comment explaining that the estimator is a conservative *lower* bound is the kind of thing that stops a future maintainer from "optimizing away" the margin.
- **`Worker::drive` process supervision.** Separate drain threads for stderr and a separate pump for stdin mean no pipe-buffer deadlock in either direction — both proved by tests pushing 400 KB through each. Stall watchdog, wall clock and cooperative stop checked in one loop with a 250 ms cancel poll. The Windows Job Object test actually spawns a grandchild and asserts it dies. The whole Claude argv is frozen byte-for-byte in a test that says *why*.
- **`Store`'s writer actor.** Single writer thread, per-run sequence recovered from `MAX(seq)` at open so seq survives restart without a gap, `PRIMARY KEY (run, seq)`, a partial unique index enforcing one live estimate per task, a final ledger row that deletes the estimate it supersedes, a reader pool deliberately *not* cleared on checkpoint (with a test explaining the 0.9 ms cost of doing so), `wal_checkpoint(TRUNCATE)` on open, drop and after every command, and a migration ladder using `BEGIN IMMEDIATE` + rollback per step that backfills FTS for pre-V2 rows. *It is correct within one process, which is its contract* — the gap is that nothing enforces one process.
- **`Plan::validate`.** Duplicate ids, unknown roles, empty briefs, self-dependency, dangling dependency, a Kahn topological sort for cycles, and a hard `MAX_TASKS = 12`. The plan schema is generated from the live `REGISTRY` so the role enum cannot drift. A genuinely defended trust boundary against the director's output.
- **The `Inconclusive`-first verify contract.** Missing report → inconclusive; stale report deleted before the run; crash exit codes → inconclusive; empty body → inconclusive in all four parsers; unknown format → inconclusive rather than a guess. Exactly one `unreachable!`, provably guarded. (The verdict *discipline* survives §3.6 and §3.7 untouched — those are the timeout path and the parsers, both downstream.)
- **The MCP trust boundary, where it is reachable.** `StoreTools::capsule_submit` rejects a capsule naming a different task than the CLI arg, and stores `from_role` from the daemon's own knowledge rather than the worker's self-declared `from`. `capsule::render` truncates in a documented order, never drops `do_not_revisit` at any step, and returns `IrreducibleOverflow` rather than silently cutting. **The hard part is already built and tested — it is simply not attached.**
- **`execute_parallel`'s structure.** Panic containment per node, account-exhaustion halting that stops dispatching workers that can only refuse, redo semantics with note plumbing, and resume that rehydrates capsules of already-done dependencies.
- **Small things done right.** `account_is_out_of_allowance` requires two independent signals so a brief *about* rate limits cannot halt a run (with the false-positive test); `guard_nested_session`; `readable_image` canonicalizes and confines to the project root; `git::reset_hard` validates the sha shape and uses argv, never a shell; credential-carrying remote URLs refused; `resume::path_for` sanitizes the project id with a traversal test; `Desk`'s `Drop` guard emits `worker_exited` only if `worker_spawned` was emitted; crash reports redact paths and usernames.
- **The tests are written as arguments.** Several state, in the assertion message, the failure they exist to prevent. Both reviewers called this out unprompted as unusual and valuable.

---

## 6. Recommended order of work

Merged from both final rankings, which agreed on the top items and differed mainly on where authentication sits.

### First — two control-flow fixes, roughly five lines
1. **Clear `stopping` at the top of `run_command`** (`studio.rs:61`). Removes a silent, persistent, user-facing dead-studio state. (§3.1)
2. **Replace `rx.recv()` with `recv_timeout` + stop polling** in `spend_approved`, `after_wave`, `propose`. Removes a permanent daemon-wide wedge. (§3.2)

Highest consequence-per-line in the entire review. Do these regardless of what else is scheduled.

### Second — make the money real
3. **One `admit()` chokepoint inside `run_worker_inner`**, covering workflow nodes, repair rounds, standalone tasks and meetings — all of which bypass admission today. Make it the place the degradation step is actually *applied* (`Effort::downshift` and `model_for_step` are built and tested, one parameter away). Fix the ratchet to advance only when a step failed to reduce the spend rate. (§2.3)
4. **Charge the run for workers that fail** — record `billed_tokens` and `cost_usd` on the `Err` path, accumulate usage across messages instead of overwriting, and estimate cost via `usd_mirror` when the terminal result never arrived. While in that file, test `saw_result` before `stalled`. (§2.4, §3.10)

### Third — restore the cache economics
5. **Stagger cold starts per prefix within a wave.** Group by `prefix_hash` (not `node.role`), release one worker per unseen hash, wait for its first streamed token, then fan out; key `warmed` on `(prefix_hash, instant)` with the 1-hour TTL. `Store::cache_health` will confirm it immediately. (§2.2)
6. **Move engine profile prose into frozen L1** and drop the per-node hint. Removes a per-spawn uncached cost *and* a charter contradiction. (§2.6)

### Fourth — fix the verify spine as one unit
7. Give `run_command` the `ProcessGroup` that already exists in `studio-core`, and abandon rather than join reader threads after a kill. (§3.6)
8. Attach gates after every leaf or after the final wave, not `wf.nodes.last()`. (§3.4)
9. Re-install or hash-check helpers immediately before **each** gate; treat a mismatch as a named Fail. (§3.3)
10. Make "no positive evidence" mean Inconclusive: `cases == 0` guard in the UE parser, depth guard in NUnit3, anchor `looks_like_infrastructure` and run it *after* failure extraction, rename or fix the test at `parsers.rs:744`. (§3.7)
11. Bind the missing substitutions — or delete the unrunnable scopes from the profiles so they fail honestly at parse time rather than silently at run time. (§3.5)

### Fifth — close the agentic loop
12. **Wire MCP into `run_worker_inner`** exactly as `main.rs:454-467` already does; append the qualified capsule tool to `allowed_tools` (accepting one cold prefix write per role, once); replace the fabricated `CapsuleSubmitted`; put rendered capsule *content* into dependent briefs in place of the dangling `cap_t1` ids. **Ship with `busy_timeout` set on every connection**, and reconcile the three inconsistent index DB paths. (§2.1)

### Where the reviewers disagreed on priority
**Authentication.** Opus ranked a session token second overall; Fable placed it after the first five. The deciding factor is your threat model: if the studio only ever runs on a single-user desktop with no untrusted local software, Fable's ordering is defensible. If workers with `Bash` are considered part of the threat model — and §4's escalation chain argues they should be — Opus is right and this moves up. **At minimum, scope `GET /ws` to require a `run` parameter now**; that one change breaks the escalation chain without waiting for a token scheme.

---

## 7. What only running the code can settle

Both reviewers flagged these as beyond static analysis. Listed so nobody treats them as established:

- **Does the `capsules` table actually stay empty?** Opus made a falsifiable prediction: run `studiod studio`, issue one build, then `SELECT count(*) FROM capsules` — it predicts zero. One command settles §2.1 definitively.
- **Is `message_start` usage per-message or cumulative across a multi-turn session?** The repo's captured fixture (`stream-real.ndjson`) is single-turn. If cumulative, the undercount in §2.4 shrinks to the output side — though `cost_usd: 0.0` on kills is unconditional either way.
- **Does the CLI deduplicate cache writes server-side across concurrent identical requests?** If so, §2.2's 3.5× headline shrinks to "the second and later workers pay full input price" — still a real loss, different number. Settled by running a two-node same-role wave and reading `cache_creation` in `token_ledger` for both rows.
- **Does Godot headless print "Vulkan" in normal operation?** Determines whether §3.7's infrastructure misclassification fires in practice or only in theory.
- **Does `quick_xml` return a clean `Eof` on unclosed documents?** The existence of `parse_junit`'s manual depth counter is strong circumstantial evidence, but it is inference.
- **How often do real director plans produce ≥2 topological leaves?** §3.4's bug is structurally certain; its frequency is not.

---

## Appendix: how the two reviews compared

Both reviewers conceded substantially, which is the main reason to trust what survived.

**Opus conceded seven findings to Fable:** the stale stop flag, the blocking `recv()` wedge, the ladder ratchet, forgeable gate evidence, `cost_usd: 0.0` on kills, `Stalled` checked before `saw_result`, and `CapsuleSubmitted` firing before the outcome check. It also accepted that Fable's framing of two shared findings was sharper than its own.

**Fable conceded nine to Opus:** unbound placeholders (its largest miss — it had framed Unity/UE5 as merely "unprobed"), gate leaf-pinning, the verify timeout hang, the parser false-Pass cluster, the Unix orphan tree, the stall watchdog, shared-tree lost updates, resume atomicity, and the budget bypass at every non-workflow spawn site.

**Retractions and corrections:** Opus partially retracted its "inverted test" framing (deliberate behaviour under a stale name, not an accidental blessing), and had four claims narrowed by Fable — positional `Failure.id` (overstated; helper failures carry stable file-path ids and no production dedup exists to defeat), the `MeetingSpend` citation (misread — `run_meeting` does pass `req.ask_above`), "no crash recovery" (overbroad — workflow-granularity resume exists and works), and skipped-tests-as-pass (standard CI semantics). Fable retracted nothing but accepted two calibrations: its ratchet worked-example does not hold under shipped defaults, and its "origin guarding above the bar" phrasing contradicted its own rebinding finding.

**Neither reviewer executed the code.** Section 7 is the honest boundary of this review.
