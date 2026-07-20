# 06: Budget Governance

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **Owns:** the budget model and the five-step degradation ladder. Reads the `token_ledger` and `budgets` tables ([03](03-state-store.md)); uses the layered-prompt and force-summarize mechanics from [02](02-context-engine.md); consumed by the supervisor's pre-spawn gate ([01](01-orchestrator-core.md)).

## Budgets are in tokens, with a USD mirror

The unit of account is **tokens**, because tokens are what the ledger measures exactly and what the subscription meters. USD is a **mirror**: derived from tokens via the model's per-MTok price ([02](02-context-engine.md) pricing, Fable 5 $10/$50, Opus 4.8 $5/$25, cache read ≈ 0.1×, cache write ≈ 1.25×) for display and reporting only. Enforcement always compares tokens to token limits; the USD number never gates anything, because prices can move and cache accounting makes per-request USD lumpy.

Two scopes, both in the `budgets` table ([03](03-state-store.md)):

- **Task budget**: a ceiling for one task and its repair rounds/consults. Sized from the role's tier and the workflow node.
- **Sprint budget**: a ceiling for a whole run/sprint across all its tasks. The task budgets roll up into it.

`spent_tokens` is maintained from the realtime-spend query ([03](03-state-store.md)): final ledger rows are authoritative, in-flight workers contribute their latest interim estimate.

## Enforcement at three points

| Point | Check | On breach |
|---|---|---|
| **Pre-spawn** | Would this worker's *projected* input (frozen prefix size + L3 size, both known before spawn) plus a per-role output reserve fit under both the task and sprint remaining? | Refuse or degrade before paying anything ([01](01-orchestrator-core.md) consults this gate) |
| **In-flight** | Interim `token_usage` estimates (or EMA fallback, see below) crossing a soft threshold | Emit `budget_warning`; arm the degradation ladder |
| **Capsule time** | On `capsule_submit`, the now-known task spend against the task budget | Apply the next ladder step for the next task in scope |

### In-flight estimation and the settle-at-`result` fallback

Whether streamed events carry usable interim `usage` deltas is **unverified** ([00](00-overview.md), [13](13-risks.md)). The design does not depend on the answer:

- If interim deltas exist, they drive in-flight enforcement directly.
- If they don't, the enforcer uses an **EMA-based estimate**: an exponential moving average of output rate per role, seeded from historical ledger rows, to project in-flight spend. **The estimate always settles to the exact number at the terminal `result` event**, which writes the authoritative `estimate=0` ledger row and supersedes every interim estimate. So the worst case is a slightly stale in-flight number that becomes exact the moment the worker exits, never a wrong charge.

## The five-step degradation ladder

Applied in order as a scope approaches its limit. Each step is cheaper than the one before it and emits `degradation_applied` ([05](05-event-protocol.md)) with the step number. The ladder is per-scope; a sprint nearing its cap degrades every task under it.

```mermaid
graph TD
  S0[under budget: run normally] --> S1
  S1["1. Effort downshift<br/>drop role effort one band (never below floor, 04)"] --> S2
  S2["2. Model downshift<br/>route eligible Tier-2 work to fable where the role permits"] --> S3
  S3["3. Trim L3<br/>fewer pushed ADRs, tighter symbol slice, shorter capsule caps"] --> S4
  S4["4. Force-summarize<br/>reset a bloated session into a fresh one (see below)"] --> S5
  S5["5. Hard stop<br/>no new spawns in scope; finish in-flight; escalate to studio_director"]
```

1. **Effort downshift**: lower `--effort` one band, respecting the role floor ([04](04-agent-graph.md)). Cheapest lever, smallest quality cost.
2. **Model downshift**: where a role and task permit, route to `--model fable`. Not all roles are eligible (Tier-3 systems work isn't); the registry marks eligibility.
3. **Trim L3**: the context engine tightens the volatile layer: fewer pushed ADRs (top-3 not top-5), a narrower symbol slice, lower capsule render caps. The frozen prefix is untouched, so cache warmth is preserved even while degrading.
4. **Force-summarize**: **the emphasized lever.** A long-lived session accumulates a bloated JSONL history; every `--resume` re-processes it. Force-summarize distills the session's state into a single task capsule ([02](02-context-engine.md)) and **starts a fresh session** seeded with that capsule as L3, discarding the heavy history. This collapses a session whose per-turn input has crept up back down to a clean frozen-prefix-plus-small-L3 shape, often reclaiming more budget than steps 1-3 combined, because it attacks input-token bloat at its root.
5. **Hard stop**: no new workers spawn in the scope; in-flight workers finish; the daemon escalates to `studio_director` ([04](04-agent-graph.md)) with a spend report. `budget_exhausted` fires. Nothing is silently dropped. The human sees the stop.

## Why force-summarize matters most

The other steps trade quality for tokens at the margin. Force-summarize is different: it targets the compounding cost of long sessions. Under `--resume`, a session that has run 40 turns pays to re-read all 40 turns of history on turn 41. The frozen-prefix design keeps the *prefix* cheap, but the *message history* still grows. Resetting a bloated session into a fresh one seeded with a summary is the only ladder step that reduces the structural, per-turn input cost rather than shaving a one-time slice, which is why it sits just below hard stop and is preferred over stopping whenever the work can continue.
