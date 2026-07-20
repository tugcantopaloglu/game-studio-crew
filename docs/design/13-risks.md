# 13: Risks

> **Status:** v0.2. R1 and R2 closed by M1 probes, R0 added after it materialized, R11 added for the unprobed engine profiles.
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

## Not-yet-risks (watch list)

- **Ephemeral container state** ([00](00-overview.md), [03](03-state-store.md)): both SQLite DBs are disposable and only survive restarts if committed to the project data dir. Losing them costs a re-index and a cold ledger, not correctness.
- **Subscription policy changes** (fast-mode availability, model routing): the design pins `--model fable|opus` and would need a one-line profile change if aliases move; no code assumes a specific alias beyond those two.
