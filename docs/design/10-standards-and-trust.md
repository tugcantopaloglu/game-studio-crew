# 10: Standards and Trust

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **Owns:** the three rule modes and the R0-R4 graduated trust model. References roles ([04](04-agent-graph.md)), the diff/index ([11](11-index-and-bootstrap.md)), and verification ([08](08-verification.md)). No role list or engine name is redefined here.

## Three rule modes

A "standard" is a rule the studio enforces. Each rule declares a **mode** that decides *where* and *how* it runs, and, critically, whether it costs any model tokens.

| Mode | Runs as | Token cost | Example |
|---|---|---|---|
| `lint` | an **existing external analyzer** the daemon invokes (the engine's linter, `dotnet format`, `gdlint`, clang-tidy) | **zero** | "no unused `using`s", "consistent formatting" |
| `check` | **custom Rust over the diff** ([11](11-index-and-bootstrap.md) produces the diff + symbol deltas) | **zero** | "no `Debug.Log` in shipped code", "scene files touched only via the import pipeline", "no new singletons" |
| `prompt` | **prose compiled into the charter**, scoped by zone | costs tokens (it's in L2/L3) | "prefer composition over inheritance for new gameplay systems", judgment calls a regex can't make |

`lint` and `check` are mechanical: deterministic, fast, free. `prompt` is judgment-based and the only mode that spends tokens, so it is used sparingly and **scoped by zone**: a rule that only matters in the netcode directory is compiled into the charter only for tasks touching that zone (via the same substring/zone triggering as capability fragments, [07](07-engine-layer.md)), never carried by every worker.

## Mechanical rules run in verify regardless

The key design decision: **`lint` and `check` rules run in the verification pass ([08](08-verification.md)) no matter what.** A `prompt`-mode restatement of a mechanical rule is a **cost optimization, not the enforcement mechanism**: it nudges the agent to get it right the first time and save a repair round, but the mechanical check in verify is what actually enforces it. This means:

- A rule is never *only* a prompt. If it can be checked mechanically, the mechanical check is authoritative and always runs.
- Prompt injection of a mechanical rule can be dropped under budget pressure ([06](06-budget-governance.md) step 3, trim L3) **without weakening enforcement**: the verify-time check still fires. You lose a little first-pass accuracy, not a guarantee.
- A worker cannot talk its way past a `check` rule; it's Rust over the diff, not a request.

So the enforcement guarantee lives in verify; the prompt layer only buys fewer repair rounds.

## R0-R4 graduated trust

Trust is keyed on **realized diff blast radius**, how much, and how dangerously, a change actually touched, computed from the diff and symbol deltas ([11](11-index-and-bootstrap.md)) *after* the work, not predicted before it. Higher tiers require more gating.

| Tier | Realized blast radius | Gate added |
|---|---|---|
| **R0** | trivial: comments, strings, isolated constant, single-symbol body, fully covered by a passing test | verify only; auto-advance |
| **R1** | local: one file, no public signature change | verify + `check` rules |
| **R2** | cross-file: public signature change, new dependency edge ([11](11-index-and-bootstrap.md) refs) | + peer consult ([04](04-agent-graph.md) horizontal) |
| **R3** | structural: touches shared systems, many refs, or a data/scene file | + senior review (`escalates_to`) before merge |
| **R4** | dangerous: build config, save format, binary asset, anything the diff can't be reviewed as text | + explicit human/`studio_director` approval ([09](09-workflows.md) approval gate) |

The tier is assigned by the daemon from the realized diff, so an agent that *intended* a small change but produced a sprawling one is gated at the sprawl's tier, not the intent's. Tier assignment emits nothing new. It parameterizes the existing gates ([09](09-workflows.md)).

## Binary `.umap` auto-escalation

UE `.umap`/`.uasset` are binary ([07](07-engine-layer.md), [13](13-risks.md)): a diff tool cannot show a human *what* changed, only *that* it changed. Because text review is impossible, **any `.umap` change auto-escalates one trust level**: an otherwise-R2 change touching a `.umap` becomes R3, an R3 becomes R4. The rationale is explicit: the review that a lower tier relies on (reading the diff) doesn't exist for a binary map, so the system compensates by demanding the stronger gate. This is the one place the trust model hard-codes an engine-specific fact, and it is isolated here rather than leaking into role charters ([04](04-agent-graph.md)).
