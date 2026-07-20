# 13: Risks

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> Consolidated risk register. Each risk names where it's addressed and its mitigation or fallback. This is the honest list of what could break.

## The two unverified CLI behaviors (settled first in M1)

These are the only risks the whole architecture is *structurally* exposed to, which is why M1 exists to settle them before anything is built on them ([00](00-overview.md)).

| # | Risk | Impact | Fallback (already designed) |
|---|---|---|---|
| R1 | **`--mcp-config` may not attach under `--bare`.** Capsules and orchestrator callbacks depend on the worker reaching the daemon's stdio MCP. | Workers can't submit capsules the primary way. | **Watched outbox directory** ([00](00-overview.md), [02](02-context-engine.md)): workers write capsules to a file the daemon watches via `notify`. Schema validation and truncation are unchanged; only the transport differs. |
| R2 | **Streamed events may not carry usable interim `usage` deltas.** In-flight budget enforcement wants live token counts. | In-flight enforcement is blind mid-worker. | **EMA-based estimation that settles to exact at the terminal `result`** ([06](06-budget-governance.md)): worst case is a slightly stale in-flight number, never a wrong charge. |

Both fallbacks are fully specified, so a "no" on either is a known, bounded degradation. Not a redesign.

## Upstream / opaque risks (we don't control these)

| # | Risk | Where | Mitigation |
|---|---|---|---|
| R3 | **Opaque subscription rate limits.** The TPM/RPM ceiling is not published and can change. | [01](01-orchestrator-core.md) | **AIMD token bucket** probes for headroom (additive increase) and backs off on 429 (multiplicative decrease); self-heals when the limit shifts. No hard-coded limit to be wrong about. |
| R4 | **Cache opacity upstream of our prefix.** We control our prefix bytes, but caching behavior (eviction, exact minimums, cross-process warmth) is the provider's. | [02](02-context-engine.md) | Byte-stability rules + padding past the confirmed minimums (Opus 4.8 4096, Fable 5 2048); the **per-role `cache_hit_ratio`** ([03](03-state-store.md)) turns a caching regression into a visible, alarmed metric rather than a silent cost leak. We measure, we don't assume. |

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
