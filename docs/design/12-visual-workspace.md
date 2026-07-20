# 12: Visual Workspace

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **Consumes** the event protocol ([05](05-event-protocol.md)) and the role registry ([04](04-agent-graph.md)). The **event → visual mapping table** below has **exactly one row per event type in the [05](05-event-protocol.md) enum, and no rows for types not in it**: that 1:1 parity is verification check #2 for this phase.

The studio floor is the answer to "the studio is invisible while it works" ([00](00-overview.md)). It is a browser view that renders the daemon's event stream as a top-down office where you can watch the crew.

## Projection: a 3D voxel scene

> **Superseded section.** The floor is now rendered in 3D with voxel characters ([ADR 0005](adr/0005-voxel-3d-floor.md)): Three.js, isometric-style camera by default, free orbit on a keypress, one `InstancedMesh` of ~290 unit cubes per character. The spatial model below is unchanged and still drives it; only the renderer differs. The original 2D reasoning is kept for the record.

### Original decision: top-down orthogonal grid, 32px tiles, deliberately not isometric

The floor is a **top-down orthogonal grid at 32px tiles**, not isometric. The reasoning ([ADR 0003](adr/0003-top-down-not-isometric.md)):

- **Hit testing** is trivial in orthogonal (screen→grid is a divide); iso needs inverse projection and depth sorting.
- **Text placement** (desk labels, hover bubbles, speech) sits flat and legible; iso skews text or forces billboarding.
- **CC0 asset availability**: top-down 32px tilesets (Kenney and similar) are abundant; good iso office sets are rare.

The spatial model (grid coordinates, rooms, desks) is **projection-independent**, so a later switch to iso is a renderer change, not a data-model change. The store, the packing, and the event mapping are unaffected.

## Deterministic floor generation

The floor is **generated from the role registry ([04](04-agent-graph.md)), not hand-drawn.** A **deterministic shelf-packing** algorithm lays out department rooms and desks from the 13 roles: rooms are packed by department, desks within a room by role. Because packing is deterministic and keyed on the registry, **adding a role never redraws the existing map**: it places one new desk in its department room; every other desk keeps its coordinates. This is the visual counterpart to "the registry is the only place roles are defined" ([04](04-agent-graph.md)).

## Avatar state language

An avatar encodes three orthogonal facts with three orthogonal visual channels, so nothing is ambiguous:

- **Shape encodes tier** (1/2/3): headgear and silhouette in the voxel model; you can read seniority at a glance.
- **Fill encodes department**: the six department colors ([04](04-agent-graph.md)).
- **Only the status ring encodes runtime state**: idle / running / blocked / meeting / error. Runtime churn touches *only* the ring, so a worker starting and finishing never changes its shape or color, only the ring animates. This separation is what keeps the floor readable under load and is the last thing to degrade ([§performance](#performance-budget-and-degradation)).

## Desk PC screens as RenderTextures

Each desk has a small PC screen showing what that worker is doing (current tool, a scrolling snippet). These are **RenderTextures** with a **three-band LOD**:

- **Far:** a static "on/off + color" texture, no per-frame update.
- **Mid:** a low-rate icon/summary update.
- **Near (camera focused):** live-ish content.

A **hard cap of 6 texture updates per frame** bounds GPU cost regardless of crew size. The renderer round-robins which near/mid screens get their update budget each frame, so 13 busy desks never blow past 6 updates.

## Hover bubble: DOM overlay

Hovering an avatar shows a bubble with the **live distilled summary** (the turn digest / capsule `summary`, [02](02-context-engine.md)). It's a **DOM overlay**, not a canvas draw, so it gets real text layout, wrapping, selection, and accessibility for free, positioned over the canvas at the avatar's screen coordinate.

## Meeting choreography

Meetings ([04](04-agent-graph.md): delegation, consultation, escalation, arbitration) are shown as **choreography**, not just a log line:

- Participants **walk to convergence** at a meeting spot (a table for arbitration, a desk-side for a consult).
- A **whiteboard** shows the meeting's artifact (the decision under discussion, the failure list).
- **Single-speaker bubbles**: one participant "speaks" at a time so the exchange is legible.

Choreography is driven entirely by `meeting_started`/`meeting_ended` and the collaboration events; the client needs no domain logic because the `scene.meeting` block ([05](05-event-protocol.md)) carries participants and room.

## Camera, minimap, timeline scrubber

- **Camera zoom bands:** discrete zoom levels tied to the screen LOD bands (zooming in promotes desks to near-LOD).
- **Follow mode:** lock the camera to one avatar and watch it work/meet/move.
- **Minimap:** the whole floor with status-ring colors, for at-a-glance "where's the red ring."
- **Timeline scrubber:** scrub the run's history. **This works precisely because the store is a pure reduction over the event log** ([05](05-event-protocol.md)): the floor state at time *T* is `reduce(events where seq ≤ seq_at(T))`. Scrubbing is re-reducing to an earlier `seq`: no special history format, no snapshots to maintain, because the event log *is* the history.

## Event → visual mapping

**One row per [05](05-event-protocol.md) event type, exactly.** The client is a lookup over this table; it holds no domain rules.

| Event ([05](05-event-protocol.md)) | Visual effect on the floor |
|---|---|
| `run_started` | floor resets/highlights; run banner appears |
| `run_ended` | run banner closes; final spend readout shown |
| `worker_spawned` | avatar appears/activates at its desk; ring → running |
| `worker_state_changed` | status ring updates (running/blocked/meeting/error/idle) |
| `worker_exited` | ring → idle; desk dims; PC screen → far-LOD |
| `tool_call` | PC screen shows the tool icon; brief desk pulse |
| `tool_result` | PC screen updates with ok/err tint |
| `prompt_frozen` | subtle "charter loaded" glyph at the desk (dev/debug overlay) |
| `cache_hit` | green "cache" spark on the desk. The token-thrift tell |
| `token_usage` | desk spend meter increments; feeds the run spend readout |
| `summary_created` | a "notes" glyph floats to the department shelf |
| `task_delegated` | arrow/walk from parent desk toward child desk |
| `task_returned` | capsule glyph travels back to the parent desk |
| `consult_requested` | dashed sideways line to the consultant's desk |
| `consult_answered` | consultant's reply glyph returns; consultant desk releases |
| `escalated` | upward arrow to `escalates_to`; escalating ring flags |
| `capsule_submitted` | capsule glyph emitted from the desk (color by kind) |
| `meeting_started` | participants walk to convergence; whiteboard appears |
| `meeting_ended` | participants disperse; whiteboard result flashes |
| `decision_recorded` | ADR card pinned to the department/wall board |
| `verify_started` | a "test bench" spins up near infra; progress marker |
| `verify_result` | pass = green check, fail = red list, inconclusive = amber |
| `repair_round` | round counter ticks on the failing worker's desk |
| `inconclusive_flagged` | amber ticket slides to the infra queue lane |
| `budget_warning` | run spend readout turns amber; soft chime glyph |
| `degradation_applied` | a "throttle" badge (step number) on affected desks |
| `budget_exhausted` | run spend readout turns red; hard-stop banner |
| `workflow_started` | a workflow track/lane appears across the floor |
| `node_entered` | the active node lights on the workflow track |
| `gate_evaluated` | gate marker on the track flips pass/fail |
| `workflow_ended` | workflow track closes with its outcome |
| `index_updated` | a brief "index" pulse at the library/shelf; cache-health tint if `cache_hit_ratio` dips ([03](03-state-store.md)) |

## Art and assets

Programmatic art for the floor/desks/rings (drawn from primitives so it scales and recolors deterministically), **Kenney CC0** furniture tilesets for props, and **Lucide** icons for tools/status glyphs. All CC0/permissive; nothing blocks a self-contained build.

## Performance budget and degradation

Target 60fps with the full 13-desk crew active. A **degradation ladder** sheds cost under load, and **status-ring colors and blocked indicators are the last things to degrade**, because they carry the information the whole floor exists to convey:

```
1. Drop PC-screen updates to far-LOD everywhere (keep the 6/frame cap unused)
2. Freeze meeting choreography to static poses + whiteboard (no walking)
3. Stop prop/ambient animation
4. Coalesce glyph effects (batch capsule/cache sparks)
5. LAST: status rings and blocked/error indicators, never dropped
```

If the floor can only draw one thing, it draws which workers are stuck. Everything else is affordance.
