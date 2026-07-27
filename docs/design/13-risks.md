# 13: Risks

> **Status:** v0.3. R1 and R2 closed by M1 probes, R0 added after it materialized, R11 added for the unprobed engine profiles, R12-R15 added when the floor and the store were measured, R14 closed when the resume call sites were bounded.
> Consolidated risk register. Each risk names where it's addressed and its mitigation or fallback. This is the honest list of what could break.

## The two unverified CLI behaviors: both settled in M1, both closed

| # | Risk | Verdict | Status |
|---|---|---|---|
| R1 | **`--mcp-config` may not attach.** Capsules and orchestrator callbacks depend on the worker reaching the daemon's stdio MCP. | **Attaches.** `init.mcp_servers: [{"name":"probe","status":"connected"}]`; the tool was advertised, invoked, and returned its value. | **Closed.** The watched-outbox fallback is specified but **not built**. It returns to the table only if MCP attachment regresses. |
| R2 | **Streamed events may not carry usable interim `usage` deltas.** In-flight budget enforcement wants live token counts. | **They carry them.** `stream_event`/`message_start` has a full `usage` block; four pre-`result` events carried usage in a short turn. | **Closed.** The EMA fallback is specified but **not built** ([06](06-budget-governance.md)). |

## R0: the risk that materialized

| # | Risk | Where | Outcome |
|---|---|---|---|
| R0 | **`--bare` is incompatible with subscription auth.** It reads auth strictly from `ANTHROPIC_API_KEY`/`apiKeyHelper`; OAuth and keychain are never read. The design named it "the primary token lever" across four documents and an ADR. | [00](00-overview.md), [01](01-orchestrator-core.md), [02](02-context-engine.md), [04](04-agent-graph.md), [ADR 0001](adr/0001-claude-cli-as-worker.md) | **Materialized, and was not on any risk list.** Resolved by [ADR 0004](adr/0004-explicit-context-control-not-bare.md): explicit context control (`--setting-sources ""`, `--system-prompt-file`, `--tools`) reaches a lower token floor than `--bare` promised, with OAuth intact. |

R0 is listed after the fact because the lesson is the register's most valuable entry: **the risk that hurt was the one recorded as a verified fact.** `--bare` was verified as *documented* behavior and never executed. Every CLI fact in [00](00-overview.md) is now probe-measured, and the standing rule is that a fact the architecture rests on is unverified until a probe has run it and the exit code has been read.

## Upstream / opaque risks (we don't control these)

| # | Risk | Where | Mitigation |
|---|---|---|---|
| R3 | **Opaque subscription rate limits.** The TPM/RPM ceiling is not published and can change. | [01](01-orchestrator-core.md) | **AIMD token bucket** probes for headroom (additive increase) and backs off on 429 (multiplicative decrease); self-heals when the limit shifts. No hard-coded limit to be wrong about. |
| R4 | **Cache opacity upstream of our prefix.** We control our prefix bytes, but caching behavior (eviction, exact minimums, TTL, write premium) is the provider's and can move without notice. | [02](02-context-engine.md) | Partly retired by measurement: cross-process warmth is **confirmed** (17.4× warm), TTL is **1 hour**, the write premium is **2.0×**. The minimums (Opus 4096, Fable 2048) remain **documented but unprobed**, so padding stays in. The **per-role `cache_hit_ratio`** ([03](03-state-store.md)) turns any regression in the provider's behavior into a visible, alarmed metric rather than a silent cost leak. A TTL or premium change would show up there first. |

## The unprobed engine profiles

| # | Risk | Where | Status |
|---|---|---|---|
| R11 | **The Unity and UE5 profiles have never been executed.** Their command lines, report paths and exit-code semantics are read from documentation, which is exactly the class of assumption that R0 and [ADR 0004](adr/0004-explicit-context-control-not-bare.md) were written about. | [07](07-engine-layer.md) | **Open.** Godot is probed and was wrong in three ways when run. The mitigation is not a fallback, it is to run them: install the engine, execute each of the five commands, read the exit codes, and correct the profile. Until then the profiles are marked unprobed in [07](07-engine-layer.md) rather than presented as fact. |

The verification layer limits the blast radius: a command that produces no
report, an unreadable report, or an unrecognised schema returns `Inconclusive`
and routes to infra rather than being guessed as a pass ([08](08-verification.md)).
So a wrong profile shows up as work that will not verify, not as a green build
that is actually broken. That is a containment, not a fix.

## Engine-specific risks

| # | Risk | Where | Mitigation |
|---|---|---|---|
| R5 | **UE binary asset and Blueprint blindness.** `.umap`/`.uasset` and Blueprints are binary; diffs can't show *what* changed, and syntactic indexing can't read them. | [10](10-standards-and-trust.md), [11](11-index-and-bootstrap.md) | Asset-registry dumps (coarse, debounced) instead of binary parsing; `.umap` changes **auto-escalate one trust level** so a change no human can review as text gets the stronger gate. |
| R6 | **UE automation report schema drift across 5.x.** The JSON report shape changes between minor versions. | [08](08-verification.md) | The `ue_automation_json` parser is **defensively coded**: reads known fields, tolerates renames/absences, and returns **`Inconclusive`** (→ infra queue) rather than guessing a pass on an unparseable shape. |
| R7 | **Unity editor lock serializes verify to ~one concurrent op per project.** The editor holds an exclusive project lock. | [07](07-engine-layer.md), [01](01-orchestrator-core.md) | Acknowledged as a throughput ceiling, not a bug; the scheduler serializes Unity `test_full`; **Windows Job Object reaping** guarantees a killed worker never leaves a lock-holding editor orphaned to wedge the queue. Godot (no lock) is the M3 target for exactly this reason. |

## Structural / quality risks

| # | Risk | Where | Mitigation |
|---|---|---|---|
| R8 | **Tree-sitter refs are syntactic only.** No type resolution, so `refs` has false and missing edges. The "call graph" is a hint, not ground truth. | [11](11-index-and-bootstrap.md) | Consumers treat refs as a strong hint; the trust model's cross-file tiering ([10](10-standards-and-trust.md)) tolerates false edges by gating conservatively (over-gating is safe, under-gating isn't). Verify ([08](08-verification.md)) is the real correctness check, not the ref graph. |
| R9 | **Headless testing has no visual coverage.** All verification is headless; a build that compiles, passes tests, and cooks can still look wrong. | [08](08-verification.md), [07](07-engine-layer.md) | Explicitly out of scope for automated verify; the `approval` gates ([09](09-workflows.md)) and R3/R4 human review ([10](10-standards-and-trust.md)) are where visual/subjective judgment enters. The studio doesn't claim to verify *feel*. |
| R10 | **Capsule quality is a prompt-engineering dependency.** A bad `summary` or `handoff` degrades every downstream actor, and prompt quality isn't guaranteed. | [02](02-context-engine.md) | **Schema validation is the backstop**: kind, required fields, token caps, and truncation order are enforced mechanically even if the prose is weak. A capsule can be *thin*, but it can't be *malformed* or *unbounded*. The `do_not_revisit` field and repair loop limit the blast radius of a weak capsule. |

## Performance risks on the floor and in the store

| # | Risk | Where | Mitigation |
|---|---|---|---|
| R12 | **Toggling a light's visibility recompiles every shader in the scene.** Three.js puts `numPointLights` and `numSpotLights` in the program cache key, and `projectObject` drops invisible lights from the count, so a worker starting or stopping used to change the count and invalidate the program for all 135 materials at once. | [12](12-visual-workspace.md) | The desk spot light and the stuck-worker alarm are now **always in the scene and always visible**, driven by `intensity`, so the count is constant at 22 point plus 13 spot for the whole session. The look is unchanged. **The rule this leaves behind: never toggle `visible` on a light on the floor, ever; drive `intensity` instead.** The low-spec tier does not create those lights at all, so its count is a constant zero. |
| R13 | **The static half of the scene graph is frozen and will silently refuse to move.** `fixtures` (rooms, walls, desks, props, shell, whiteboard) has `matrixAutoUpdate` and `matrixWorldAutoUpdate` set false after one forced update, which is what keeps 596 of 1106 objects out of the per-frame matrix walk. | [12](12-visual-workspace.md) | Anything that needs to animate must be parented to `world`, not `fixtures`, or it will hold its build-time transform for ever with no error. `buildOffice` returns `fixtures` explicitly so this is visible at the call site, and `probes/floor-smoke.mjs` asserts the freeze is in place so it cannot be lost by accident. |
| R14 | **`events_since(run, 0)` on every snapshot, resume and websocket connect.** The store's SQL is bounded by `PRIMARY KEY (run, seq)`, but three call sites in `crates/studio-server/src/lib.rs` read the whole run and filtered in Rust. Measured over HTTP against a 50000-event run: **139.9ms to answer "nothing new" with 64 bytes**, and a websocket reconnect wanting 100 events got its first frame at 139.4ms. It grew with the run for the life of the run. | [03](03-state-store.md), [05](05-event-protocol.md) | **Closed.** `head_seq(run)` costs 0.055ms and the call sites now ask `plan_resume` what they need before reading anything: `UpToDate` reads nothing, `Replay` reads only its slice, `Snapshot` still reads the whole log because it genuinely needs it. Same measurement after: **0.26ms** for the 64-byte answer and **1.5ms** to the first frame of a 100-event reconnect. The read connections are pooled with cached statements rather than opened per call. |
| R14a | **The store reads exclusively and the resume plan counts inclusively.** `events_since` is `seq > ?2` while `plan_resume` returns `from_seq = since_seq + 1`, so passing `from_seq` straight through drops exactly one event from every replay and every reconnect — a gap that would surface much later as an unexplained hole in a run. | [03](03-state-store.md), [05](05-event-protocol.md) | The call sites pass `from_seq.saturating_sub(1)`, saturating rather than subtracting because the "`from_seq` is always at least 1" invariant lives in another function and a `u64` underflow panics in debug. Pinned by `a_replay_contains_the_event_whose_seq_is_from_seq_because_the_store_read_is_exclusive` and by `two_resumes_either_side_of_a_cut_reproduce_the_log_with_no_gap_and_no_repeat`, both of which fail if the adjustment is removed. |
| R14b | **A frame-skip guard that has never run at its own skip rate.** The distant-rig throttle added `this.owed += dt` on a field the constructor did not initialise; `undefined + dt` is `NaN`, and `NaN < minStep` is false, so the guard does not freeze the rig — it passes through once with `dt = NaN` and permanently poisons `sit`, `phase`, `yaw` and every joint transform, because `approach(NaN, ...)` stays `NaN` for ever. The avatar then renders from `NaN` matrices. | [12](12-visual-workspace.md) | Caught in review before it shipped. The field is initialised, and the case is pinned by a check that drives a rig at 15Hz for 60 frames and asserts every joint stays finite. **The general lesson: `farRigPeriod` is 0 at the default tier, so the throttle branch never executes there. A tier-gated code path must be exercised at its own tier.** The floor probe now reports how many rig updates took the throttled path — 0 of 21600 at the default tier, 18000 of 18000 at low spec. |
| R15 | **Nothing about the floor's GPU cost has been measured, only its CPU cost and its shape.** The scene submits **604 renderable meshes and 472098 triangles, of which 361 meshes and 450816 triangles are drawn a second time into a 4096x4096 soft shadow map**, under 35 dynamic lights. Those are counts, taken by walking the built graph headlessly; no frame has ever been timed on a real GPU in this repo. | [12](12-visual-workspace.md) | The low-spec tier attacks all three terms at once (shadows off, pixel ratio 1, no point or spot lights) and is offered automatically when the measured 95th-percentile frame time passes 26ms over 240 frames, so the machine reports its own verdict rather than the design guessing. The default tier's draw-call count is **unreduced**: merging the static room geometry would cut it, and was not attempted because there is no way to confirm the win here. |

| R16 | **The floor never asked for a GPU.** `new THREE.WebGLRenderer({ antialias })` left `powerPreference` at its default of `'default'` (`vendor/three.module.js:28472`), so on a hybrid-graphics laptop the browser was free to hand the floor the integrated GPU, and nothing in the page could tell whether it had been given a hardware context or a software one. | [12](12-visual-workspace.md), [14](14-settings-and-providers.md) | `gpu.acceleration`, **default true**, now asks for `powerPreference: "high-performance"` with `failIfMajorPerformanceCaveat: true`, and retries once without the caveat flag if the browser refuses. A refusal is the definitive at-creation-time signal that the context is software, and it offers the low-spec tier immediately instead of waiting 240 frames for the frame times to prove it. Off means `"low-power"`, for someone on battery who wants the fan to stop. **`powerPreference` is only read when the context is created, so the change takes effect on reload and the floor says so with a reload prompt rather than silently doing nothing.** |
| R17 | **Whether that flag helps has not been measured, and cannot be here.** This environment has no GPU, no display, and no connected browser (`list_connected_browsers` returns empty). Every frame number in this repo comes from a CPU-side harness that imports the real three.js and never rasterises, which cannot observe a GPU-selection flag. | [12](12-visual-workspace.md) | What **is** verified: that real three.js r160 forwards `powerPreference` verbatim into `canvas.getContext` (checked by recording the attributes the library actually requests, not by reading the source), and that the retry, the software-fallback verdict, the low-power path and the reload prompt all behave as specified. What is **not** verified is that any of it makes a frame faster. The floor reports its own answer instead: the sidebar shows which context it was granted, on which device, with the live 95th-percentile frame time beside it, so toggling the setting and reloading is a one-minute experiment on the user's own machine. |

R16's sibling risk is worth stating separately: the desktop shell builds its
WebView with no additional browser arguments (`desktop/src/main.rs:35`), so
WebView2 renders with whatever Edge defaults to. That is normally accelerated and
**there is no evidence here that it is not**, so no flag is being forced;
`wry` exposes `with_additional_browser_args` if a real report ever justifies it.
The caveat detection above works inside WebView2 as well as in a browser, so the
window will now say when it has been handed a software context.

R15 is the honest limit of this work. The CPU inside `animate()` is now 0.108ms mean and 0.132ms at the 95th percentile for the full 13-desk crew plus ambient, which is under one percent of a 16.7ms frame; everything left is the renderer, and the renderer needs a browser.

## Not-yet-risks (watch list)

- **Ephemeral container state** ([00](00-overview.md), [03](03-state-store.md)): both SQLite DBs are disposable and only survive restarts if committed to the project data dir. Losing them costs a re-index and a cold ledger, not correctness.
- **Subscription policy changes** (fast-mode availability, model routing): the design pins `--model fable|opus` and would need a one-line profile change if aliases move; no code assumes a specific alias beyond those two.
