# 05: Event Protocol

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **This document is the single source of truth for the event envelope and the event enum.** The [12 visual-workspace](12-visual-workspace.md) mapping table must contain **exactly one row per event type below, and no rows for types not below.** That parity is verification check #2 for this phase.

## Design stance

The wire format is **flat snake_case tags** in **one envelope**. The client does no domain reasoning: every event carries a self-describing `scene` block, so the floor renderer maps events to visuals with a lookup table, not logic. Versioning is **by tolerance**, consumers ignore unknown fields and unknown event types rather than erroring, so the daemon can add events without a client release.

## Envelope

Every event on the wire is exactly this shape:

```jsonc
{
  "v": 1,                    // protocol major version; bump only on breaking envelope change
  "seq": 148213,             // monotonic per-run sequence, gap-free, assigned by the event bus
  "ts": "2026-07-20T09:12:44.118Z",  // RFC3339 UTC, daemon clock
  "run": "run_01J...",       // run id (a top-level user request / workflow execution)
  "actor": "gameplay_engineer#7",  // role id + worker instance, or "daemon"
  "scene": {                 // everything the renderer needs to place the event; no domain logic client-side
    "room": "engineering",   // department room on the floor
    "desk": "gameplay_engineer#7",  // desk id, usually the actor
    "meeting": null          // meeting id when this event belongs to a meeting, else null
  },
  "type": "tool_call",       // one of the enum below
  "data": { }                // type-specific payload; consumers ignore unknown keys
}
```

Rules:
- `seq` is assigned by the single event-bus actor, so it is total-ordered and gap-free within a run. Clients detect loss by `seq` gaps.
- `ts` is for display only; **never** order by `ts` (clock is not the source of truth, `seq` is).
- `scene` is populated by the daemon from the actor's current placement so the client needs zero knowledge of the role registry.
- `data` is open. Adding a key is not a breaking change.

## Event enum

32 types, grouped. `data` payload sketched per type. **This list is authoritative**: [12](12-visual-workspace.md) mirrors it exactly.

### Run & worker lifecycle
| type | when | key `data` fields |
|---|---|---|
| `run_started` | a top-level request/workflow begins | `title`, `workflow?` |
| `run_ended` | run finishes | `outcome`, `duration_ms` |
| `worker_spawned` | a `claude` subprocess is launched | `role`, `model`, `effort`, `session_id`, `prefix_hash` |
| `worker_state_changed` | worker lifecycle transition | `from`, `to` (see [01](01-orchestrator-core.md) state machine) |
| `worker_exited` | subprocess reaped | `outcome`, `exit_code`, `usage` |
| `tool_call` | worker invokes a tool | `tool`, `args_digest` |
| `tool_result` | tool returns to worker | `tool`, `ok`, `bytes` |

### Context & tokens
| type | when | key `data` fields |
|---|---|---|
| `prompt_frozen` | a charter prefix is composed and hashed | `role`, `prefix_hash`, `layers` (L0-L2), `bytes` |
| `cache_hit` | a spawn's prefix was served from cache | `role`, `prefix_hash`, `cache_read`, `cache_creation` |
| `token_usage` | ledger updated (interim estimate or final) | `estimate` (bool), `input`, `output`, `cache_read`, `cache_creation`, `cost_usd` |
| `summary_created` | a summarization rung is distilled | `rung` (`turn`\|`task`\|`sprint`), `source`, `tokens_saved_est` |

### Collaboration (meetings & capsules)
| type | when | key `data` fields |
|---|---|---|
| `task_delegated` | parent delegates to child | `from_role`, `to_role`, `task_id`, `capsule_id` |
| `task_returned` | child returns its capsule | `task_id`, `capsule_id`, `outcome` |
| `consult_requested` | horizontal consult via forked session | `from_role`, `to_role`, `fork_of` |
| `consult_answered` | consultant returns a capsule | `capsule_id` |
| `escalated` | worker escalates up the tree | `from_role`, `to_role`, `reason`, `capsule_id` |
| `capsule_submitted` | any capsule passes schema validation | `capsule_id`, `kind`, `rendered_tokens`, `truncated` (bool) |
| `meeting_started` | delegation/consult/arbitration convenes | `meeting_id`, `kind`, `participants[]` |
| `meeting_ended` | meeting closes | `meeting_id`, `outcome` |
| `decision_recorded` | a decision/ADR is written | `decision_id`, `title`, `supersedes?` |

### Verification & repair
| type | when | key `data` fields |
|---|---|---|
| `verify_started` | daemon runs `EngineDriver::verify()` | `scope`, `engine` |
| `verify_result` | verify returns | `verdict` (`pass`\|`fail`\|`inconclusive`), `failures[]` (digests) |
| `repair_round` | daemon re-invokes session with failures | `round` (1-3), `failure_count` |
| `inconclusive_flagged` | verify was inconclusive; routed to infra | `reason`, `queued_for` (`infra_engineer`) |

### Budget
| type | when | key `data` fields |
|---|---|---|
| `budget_warning` | spend crosses a soft threshold | `scope` (`task`\|`sprint`), `spent`, `limit`, `pct` |
| `degradation_applied` | a degradation-ladder step fires | `step` (1-5), `action` |
| `budget_exhausted` | hard stop reached | `scope`, `spent`, `limit` |

### Workflow
| type | when | key `data` fields |
|---|---|---|
| `workflow_started` | a TOML DAG workflow begins | `workflow`, `nodes` |
| `node_entered` | execution enters a node | `node`, `role` |
| `gate_evaluated` | a gate is checked | `gate`, `kind` (`verify`\|`approval`), `passed` (bool) |
| `workflow_ended` | workflow terminates | `outcome` |

### Index
| type | when | key `data` fields |
|---|---|---|
| `index_updated` | the code/asset index changes | `paths_changed`, `symbols_delta`, `engine?` |

### Version control
| type | when | key `data` fields |
|---|---|---|
| `commit_recorded` | the daemon commits a worker's output to the project repo | `project`, `role`, `sha`, `subject` |

### Spend approval
| type | when | key `data` fields |
|---|---|---|
| `budget_approval_needed` | billed tokens cross the run's notify threshold and the run is waiting on a human | `approval_id`, `spent`, `threshold`, `node`, `usd` |

A run carries a notify threshold rather than a cap. Crossing it emits this
event and blocks the run until `POST /approve` answers; approving raises the
next threshold by the same step, so a long run reports in periodically. A run
started with no threshold never emits this and never blocks.

Commits are daemon work. A worker never runs git, never receives git tools, and
the subject line is composed by the daemon from the role and brief rather than
generated by a model, so recording history costs no tokens.

## Mapping from CLI NDJSON to studio events

Workers emit `--output-format stream-json` NDJSON. The daemon's per-worker reader translates:

| CLI NDJSON | Studio event(s) |
|---|---|
| `system`/`init` | `worker_spawned` (enriched with role/model/prefix_hash the daemon already knows) |
| `stream_event` `content_block_start` (tool_use) | `tool_call` |
| tool result block | `tool_result` |
| `mcp__studio__capsule_submit` call | `capsule_submitted` (+ `task_returned`/`consult_answered`/`decision_recorded` by capsule `kind`) |
| `mcp__studio__escalate` call | `escalated` |
| `mcp__studio__request_meeting` call | `meeting_started` |
| interim `usage` in `stream_event` (**if present**: unverified, [00](00-overview.md)) | `token_usage` with `estimate=true` |
| terminal `result` `usage`/`cost_usd` | `token_usage` with `estimate=false`, then `worker_exited` |

Everything else the daemon originates itself (verify, budget, workflow, index, freezing). Those events have `actor: "daemon"`.

## Transport, resume, coalescing

- **Transport:** WebSocket from daemon to browser; the daemon is the only writer. A REST `GET /runs/{run}/snapshot` returns the reduced store state (see [12](12-visual-workspace.md). The floor is a pure reduction over the event log).
- **Resume:** client reconnects with `?since_seq=N`. The bus replays `N+1..head` from the persisted `events` table ([03](03-state-store.md)).
- **Snapshot on large gaps:** if `head - since_seq` exceeds a threshold (default 5000), the server responds with a full snapshot + `head` seq instead of replaying, and the client resumes live from there.
- **Server-side delta coalescing at 10 Hz:** high-frequency, idempotent-by-latest events (`token_usage` estimates, `worker_state_changed`, `tool_call` bursts) are coalesced into at most one flush per 100 ms per `(actor,type)`. Terminal and collaboration events (`*_ended`, `decision_recorded`, `escalated`, `verify_result`, `budget_exhausted`) are **never** coalesced. They pass through immediately.

## Versioning contract

- Add a new event `type`: **non-breaking.** Old clients ignore it.
- Add a `data` key: **non-breaking.** Consumers ignore unknown keys.
- Change envelope shape or a `data` key's meaning: **breaking**, bump `v`.
- Clients MUST tolerate unknown `type` and unknown `scene`/`data` keys without erroring. This is what lets the daemon ship events ahead of the UI.
