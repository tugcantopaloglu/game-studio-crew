# 15: Guided Runs

> **Status:** v0.1, 2026-07-25, built and running. **Owns:** the three places a human can hold or steer a development run, and the plain-language plan that makes holding it worth doing. Consumes the plan schema and parallel executor ([09](09-workflows.md)), the run steering events ([05](05-event-protocol.md)), and the panel contract ([12](12-visual-workspace.md)).

## What a guided run is

An ordinary build is fire-and-forget: you type what you want, the studio director decomposes it, and the crew is spawned before you have read a word of the plan. That is fine when you trust the request and wrong when you are the person who has to live with the game.

A **guided run** is the same machinery with three human-shaped holds in it. You say where the game lives, you read the plan in the words a producer would use, and the crew does not move until you start it. While it runs you can talk into it or stop it. If you asked for it, it also stops after every step and shows you what it just did.

Nothing about this changes the token economics. The holds are the daemon waiting on a channel; a run that is waiting is paying for nothing.

## Plain language is a first-class output of planning

The director already returns a machine plan: an id, a role, a detailed brief and dependencies per task ([09](09-workflows.md)). A guided run asks it for one more field per task, `say`: **one sentence a producer would use telling a colleague what this step is, naming the thing in the game rather than the technique.**

"Draw the player and the pipes it flies through", not "artist: implement sprite atlas".

`say` is what the floor renders and what the human edits. The detailed brief stays underneath it, because the crew still needs it. A plan whose director forgot to say anything falls back to the first sentence of the brief, cut at a word boundary — worse prose, never a blank card.

## The three holds

### 1. Before anything is spawned: the plan

`POST /run/plan` starts a guided run. The director plans, the daemon emits `plan_proposed` with `editable: true`, and the command thread blocks on a plan gate keyed by `plan_id`. An old-style `POST /build` emits the same event with `editable: false` and does not block — the floor still gets to show the plan in plain language, it just has nothing to wait for.

While the run is held you can reword a step, delete one, add one, and reorder them. `POST /run/start` sends back the list you ended up with; `POST /run/cancel` throws it away before a single worker was paid for.

Reconciling a human's list with a DAG follows three rules, and they are the whole of it:

- **Order is what you see.** The revised plan's task order is the order of the list you sent.
- **Dependencies survive only backwards.** A step keeps the dependencies it was planned with, minus any that you deleted or moved after it. A step can never wait on work that now happens later, so no edit can produce a cycle.
- **A step you add waits on the step above it.** You put it there on purpose; the crew reads it that way. It has no other dependencies, so everything after it is free to parallelise as before.

Rewording a step **replaces its brief with your words**. A step you left alone keeps the detailed brief the director wrote. This is deliberate: if you rewrote the sentence, the sentence you wrote is the instruction, and a brief that still described the old thing would quietly win.

A revised plan is validated exactly like a planned one — unknown role, empty step, cycle — and a plan that will not run is refused with the reason rather than started half-formed.

### 2. Mid-run: interrupts

The daemon drains `StudioCommand`s serially on the same thread that runs the workflow. An interrupt sent as a command would sit in that queue until the run it was meant to stop had already finished, so **interrupts are not commands.** They go into a side channel on `AppState`, next to `approvals`, and the workflow host reads it.

`POST /run/interrupt` takes either a stop or a note:

- **A note** is emitted as `run_interrupted` and appended to the briefs of the next tier, above the working-directory hint and marked as outranking the planned brief where the two disagree. It is cleared once that tier finishes, so a note lands on one step and does not haunt the rest of the run.
- **A stop** ends the run at the next tier boundary with the outcome `interrupted`. Workers already in flight finish and commit; nothing new is spawned.

The host checks the channel in `before_wave`, which the parallel executor calls after it computes the ready set and **before it admits anything** — so a stop that arrives while a tier is running is honoured at the very next boundary, and a stop that arrives before the first tier costs nothing at all.

### 3. Every step: confirmation

The `run.stepConfirm` checkbox arms the third hold. A tier in the parallel executor is exactly the set of nodes with no unsatisfied dependency, so a tier boundary is the natural place for "the crew just finished something, look at it".

With step confirmation on, `after_wave` emits `step_approval_needed` — the step number, what the tier did in the crew's own `say` lines, and the three things you can answer — and blocks on a step gate. You can:

- **approve**, and the run continues;
- **approve with notes**, and the run continues with your notes folded into the next tier's briefs;
- **send it back with notes**, and the executor un-settles that whole tier, drops its capsules, and runs it again with your notes attached.

A rejected tier blocks everything downstream by construction: the nodes that depend on it are still pending, and their dependencies are no longer satisfied. Re-entering a node does not add a second row to the run report; `entered` stays a set of what ran, and `redo_rounds` counts how many times you sent something back.

**A run started without the checkbox behaves exactly as it did before.** `after_wave` returns immediately, the executor never waits, and the only difference on the wire is the extra `plan_proposed` event.

## Where each hold blocks

| hold | route that releases it | where the run is parked | what it costs while parked |
|---|---|---|---|
| the plan | `POST /run/start`, `POST /run/cancel` | the command thread, in `run_build`, before the workflow exists | nothing; no worker has spawned |
| an interrupt | `POST /run/interrupt` | not a block — a side channel read in `before_wave` | nothing; it never blocks a caller |
| a step | `POST /run/step` | the executor thread, in `after_wave`, between tiers | nothing; the tier is committed and idle |

Every one of these is a `std::sync::mpsc` channel held in a map on `AppState`, exactly the shape `approvals` already had for spend approval ([06](06-budget-governance.md)). Answering something nobody is waiting on returns `409` rather than being silently swallowed, so a double-click cannot look like it worked.

## What is deliberately not here

- **No resume across a daemon restart.** A run parked on a hold dies with the daemon. Workflow resumption ([09](09-workflows.md)) already knows how to re-enter at the first unsatisfied gate; wiring the holds into it is separate work.
- **No cap on redos.** A human sending the same tier back forever is a human problem, and a cap would arrive exactly when someone had a good reason for the fourth attempt. The spend threshold ([06](06-budget-governance.md)) is the backstop that actually bites.
- **No per-node approval.** Confirmation is per tier, because a tier is what the executor can cleanly re-run. Approving one of three parallel nodes and rejecting another would mean re-running one node against capsules its siblings already superseded.
