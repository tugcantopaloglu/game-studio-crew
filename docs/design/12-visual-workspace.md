# 12: Visual Workspace

> **Status:** v0.2, 2026-07-25. Built: the floor, the workflow track, the minimap, the scrubber and the meeting room panel all run against real events. Rendered in 3D with voxel characters rather than the 2D grid first specified ([ADR 0005](adr/0005-voxel-3d-floor.md)).
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

- **Shape encodes identity**: hair, outfit cut and the prop in hand are per-role, drawn from the reference sheet in `agent-images/`. The designer carries a flowchart pad, the QA engineer a ticked checklist, the audio designer a field recorder and headphones, the director a black suit and a gold-edged folio. Tier still reads through the silhouette, but the prop is what tells you *what this person does* at floor distance.
- **Fill encodes department**: the six department colors ([04](04-agent-graph.md)) tint the room, the desk and the status ring; the avatar's own palette is its role palette.
- **Only the status ring encodes runtime state**: idle / running / blocked / meeting / error. Runtime churn touches *only* the ring, so a worker starting and finishing never changes its shape or color, only the ring animates. This separation is what keeps the floor readable under load and is the last thing to degrade ([§performance](#performance-budget-and-degradation)).

## Avatars are rigged, not rigid

An avatar is not one mesh sliding between waypoints. It is a nine-joint rig — hips, torso, head, two arms, two thighs, two shins, plus the prop parented to the right hand — each joint an `InstancedMesh` of voxels sharing one geometry and one material.

The walk cycle is **driven by distance travelled, not by time**, which is what makes the feet plant. During the stance half of the cycle the leg group is *translated* backwards at exactly the body's speed, so the contact foot is stationary in world space no matter how fast the avatar is moving; the swing half arcs the foot forward with a bent knee. Speed itself ramps: a waypoint is approached with a deceleration curve and left with an acceleration curve, and turning is rate-limited, so nobody snaps to a heading any more.

On top of the cycle sit the small human tells — hip bob and shoulder roll on the step, breathing while idle, a slow weight shift, a head that turns toward the monitor at the desk, toward the table in a meeting and toward the direction of travel while walking, an occasional glance elsewhere, and an occasional lean back in the chair. Sitting is a real pose: thighs forward, shins down, hips dropped onto the seat.

## Thinking out loud

A worker's reasoning is streamed by the CLI as one delta per token. Putting that on the wire unbuffered would write a SQLite row and broadcast a websocket frame per token, so the daemon buffers it: text accumulates, is cut at the last sentence boundary, is capped at 200 characters and is emitted **at most once every 500ms per worker — two `agent_thought` events a second, plus one closing event when the worker finishes**. Reasoning blocks and answer text are parsed apart and tagged `thinking` / `speaking` / `done`, and a phase change never mixes one into the other's bubble.

The client shows it only for the agent the camera is on: a billboarded bubble above the head plus a block in the detail card, faded out a few seconds after the worker goes quiet. `thoughts.enabled` turns the whole channel off client-side.

## Chatter

The floor has a voice track: procedural WebAudio babble with no words — a pitched glottal source through a sweeping formant filter, three to six syllables an utterance, panned by screen position and attenuated by distance from the camera. It sounds only for agents near the camera, the focused agent, and agents actually sitting in a meeting, capped at two concurrent voices. Audio is created on the first user gesture and stays silent until then, which is the browser autoplay policy rather than a bug. `chatter.enabled` and `chatter.volume` (default **0.12** — background texture, never intrusive) control it.

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
- A **whiteboard** shows the meeting's artifact: the topic while the room sits down, then each position as it is stated, then the chair's ruling in green once it lands.
- **Single-speaker bubbles**: one participant speaks at a time so the exchange is legible. `meeting_spoke` is emitted once per speaker in order, so the board never has to guess who has the floor.
- A **room panel** holds the whole transcript alongside the 3D view, and keeps the ruling, the positions it overruled and the ADR path readable after the room disperses.

Choreography is driven entirely by `meeting_started`/`meeting_spoke`/`meeting_ended` and the collaboration events; the client needs no domain logic because the `scene.meeting` block ([05](05-event-protocol.md)) carries participants and room.

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
| `meeting_started` | participants walk to convergence; whiteboard appears with the topic; the room panel opens |
| `meeting_spoke` | the speaker's position replaces the whiteboard body and appends to the room panel transcript |
| `meeting_ended` | participants disperse; the whiteboard holds the ruling instead of clearing |
| `decision_recorded` | whiteboard turns green and shows the claim; the room panel shows claim, reason, overruled positions and the ADR path |
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
| `commit_recorded` | a commit chip lands on the workflow track; the run's commit count ticks |
| `budget_approval_needed` | the run pauses and the spend prompt opens; nothing advances until it is answered |
| `git_action` | the git panel's tree refreshes and the action reports its result inline |
| `plan_proposed` | the plan opens in plain language, each step editable, with interrupt and add-step controls |
| `step_approval_needed` | the run holds at the tier boundary and the step card asks approve, improve, or redo |
| `run_interrupted` | an interrupt bar shows what was sent into the run and which step it lands on |
| `agent_thought` | the focused agent's bubble and detail card stream the line it is reasoning through, then fade |
| `game_summarized` | the adopted game's card fills in with its mechanics glimpse |

## Art and assets

Programmatic art for the floor/desks/rings (drawn from primitives so it scales and recolors deterministically), **Kenney CC0** furniture tilesets for props, and **Lucide** icons for tools/status glyphs. All CC0/permissive; nothing blocks a self-contained build.

The crew's per-role looks were drawn from the reference sheet in `agent-images/` but are **generated, never loaded**: every avatar is a voxel list built at runtime from a palette-and-shape table, so the single-binary story holds and there is no runtime image fetch. Four sheet entries (animator, data analyst, security engineer, support specialist) have no role in the registry and are unused; `ui-ux_designer`, `senior_engineer`, `infrastructure_engineer`, `technical_artist` and `game_artist` map onto `ux_designer`, `systems_engineer`, `infra_engineer`, `tech_artist` and `artist`.

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
