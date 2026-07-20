# 09: Workflows

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **Owns:** the workflow DAG schema and the four specified workflows. Nodes bind to roles ([04](04-agent-graph.md)); edges carry capsules ([02](02-context-engine.md)); gates require verify ([08](08-verification.md)) or approval. Budgets per node draw from [06](06-budget-governance.md).

## Workflows are TOML DAGs, not prose commands

The original crew had 73 slash commands, prose recipes a human ran. Here a workflow is a **declarative TOML DAG** the daemon executes: nodes are role invocations, edges are capsule handoffs, gates are conditions that must hold to advance. The daemon owns control flow, so a workflow is inspectable, resumable, and emits structured events ([05](05-event-protocol.md): `workflow_started`, `node_entered`, `gate_evaluated`, `workflow_ended`) rather than being a transcript.

**The rule for adding a workflow: a new workflow file is justified by a new *gate structure*, not a new *topic*.** "Add a boss enemy" and "add a shop" are the same feature workflow with different briefs. One file. "Ship a release" has a different gate structure (cook + package + sign-off). A different file. If a proposed workflow has the same nodes and gates as an existing one, it is a brief, not a workflow.

## Schema

```toml
schema_version = 1
id = "feature"
title = "Feature development"

[[nodes]]
id = "design"
role = "game_designer"          # binds to a role id in 04
inputs = []                     # capsule ids from upstream edges; [] = seeded from the run brief
budget_tokens = 60000           # per-node ceiling, rolls into the sprint budget (06)

[[nodes]]
id = "implement"
role = "gameplay_engineer"
inputs = ["design"]             # receives the design node's capsule as L3 input (02)
budget_tokens = 200000

[[edges]]
from = "design"
to = "implement"
carries = "task_return"         # capsule kind (02) that flows along this edge

[[gates]]
after = "implement"
kind = "verify"                 # verify | approval
scope = "test_fast"             # a VerifyScope (08) when kind=verify
on_fail = "repair"              # repair (daemon loop, 08) | block | escalate
```

- **Nodes** name a role and a budget. The daemon spawns the worker with that role's frozen charter and the node's `inputs` capsules as L3.
- **Edges** move capsules. An edge `carries` a capsule kind; the daemon validates the upstream node produced that kind before advancing.
- **Gates** are the teeth. A `verify` gate runs [`EngineDriver::verify()`](08-verification.md) at the given scope; on `Fail` it triggers the daemon repair loop, on `Inconclusive` it routes to infra ([08](08-verification.md)), on `Pass` it advances. An `approval` gate pauses for a human or a designated senior role ([04](04-agent-graph.md)).

Gates and nodes emit events, so the floor ([12](12-visual-workspace.md)) renders workflow progress from the event log with no workflow-specific client logic.

## The four workflows

### 1. `feature`: build something new
```
design(game_designer) → implement(gameplay_engineer) → [gate: verify test_fast]
  → art_pass(artist, optional) → integrate(gameplay_engineer)
  → [gate: verify test_full] → review(qa_engineer) → [gate: approval]
```
Gate structure: fast-verify after implementation, full-verify after integration, human/QA approval before done. Most "add X" work is this workflow with a different brief.

### 2. `bugfix`: repair a defect
```
triage(qa_engineer) → [gate: verify test_fast to reproduce]
  → fix(gameplay_engineer) → [gate: verify test_fast, on_fail=repair]
  → [gate: verify test_full]
```
Distinct gate structure: a **reproduction gate first** (verify the bug exists before fixing. A failing test *is* the triage output), then a fix gate that must flip it green. This reproduce-first shape is what makes it its own workflow, not a `feature` variant.

### 3. `sprint_planning`: decompose and schedule
```
scope(producer) → decompose(producer) → estimate(systems_engineer, consult)
  → [gate: approval] → emit_task_graph
```
No engine verify at all. The gate is human approval of a decomposition. Its output is a set of downstream `feature`/`bugfix` runs, not code. Different gate structure (approval-only), so a distinct file.

### 4. `release`: cook, package, sign off
```
freeze(producer) → [gate: verify compile]
  → build(infra_engineer) → [gate: verify export]
  → smoke(qa_engineer) → [gate: verify test_full]
  → signoff(studio_director) → [gate: approval]
```
Gate structure unique to release: an **export/cook gate** ([07](07-engine-layer.md) `export`) plus a director-level approval. Binary-asset and `.umap` blast-radius rules ([10](10-standards-and-trust.md)) bite hardest here.

## Resumption

Because workflow state is nodes + gate results in the store ([03](03-state-store.md)) and each node's worker is a resumable CLI session ([01](01-orchestrator-core.md)), a crashed run resumes at the last un-passed gate rather than from the top. The daemon replays the DAG, skips nodes whose output capsules exist, and re-enters the first node without a satisfied downstream gate.
