# 14: Settings and Providers

> **Status:** v0.1, 2026-07-25. Built: settings persistence, per-tier and per-role model and effort overrides applied at spawn time, the provider abstraction, subscription limit reporting, and the music library.
> **This document is the single source of truth for:** the settings file and its keys, the precedence rule that resolves a seat, and the provider capability table. [02](02-context-engine.md) owns the cache key this document must not break; [04](04-agent-graph.md) owns the shipped role registry this document overrides.

Everything a user can change about how the studio spends their subscription lives in one flat JSON file. The daemon and the floor read the same file, so a change made in the browser is in force on the next spawn without a restart.

## The file

`.studio/settings.json`, resolved as `studio_settings::Settings::path_in(&studio_dir)`. A flat object of dotted string keys to JSON values, deliberately not nested: the floor's shared bus (`bus.js`) stores the same keys in `localStorage` under the same names, so the two halves never disagree about shape.

| Route | Does |
|---|---|
| `GET /settings` | the stored object, `{}` when the file does not exist |
| `POST /settings` | **merges** the posted object into the stored one and saves; returns the merged result |

A missing file reads as empty rather than failing, so a fresh studio needs no seeding. A file that is not a JSON object is refused loudly instead of silently becoming empty — a settings file that parsed as `[]` would quietly return every seat to the shipped defaults, which is exactly the sort of silent reversion the studio must never do.

### Keys

| Key | Value | Changes |
|---|---|---|
| `provider` | provider id | which CLI every worker spawns as |
| `provider.role.<role_id>` | provider id | that one seat's CLI |
| `models.tier1` `.tier2` `.tier3` | `fable` \| `opus` \| `haiku` | the model for every seat in that tier |
| `models.role.<role_id>` | same | that one seat's model |
| `models.<provider>.tier<N>`, `models.<provider>.role.<id>` | free text | the model name handed to a non-claude CLI |
| `effort.tier<N>`, `effort.role.<id>` | `low` … `max` | `--effort` for those seats |
| `limits.enabled` | bool | whether the panel polls `/limits` |
| `limits.refreshSeconds` | 1800 \| 300 \| 60 | how often |
| `music.enabled` `.folder` `.track` `.volume` `.shuffle` | | the floor's music |
| `lowSpec` | bool | consumed by the 3D scene |
| `chatter.*`, `thoughts.*`, `run.stepConfirm` | | owned by other panels; listed here because they share the file |

Empty strings and whitespace count as **unset**, not as a value. This is what lets the UI offer a single "inherit" option in every select without needing a separate delete verb.

## Resolving a seat

One rule, applied to every scoped key by `Settings::scoped(prefix, role_id, tier)`:

```
<prefix>.role.<role_id>   →   <prefix>.tier<N>   →   <prefix>   →   the registry
```

Role beats tier beats studio beats what shipped. `studio_agents::REGISTRY` remains the shipped default and is never rewritten; `exactly_one_seat_sits_on_fable_and_it_is_tier_one` is a statement about the registry, and it stays true no matter what the user has configured.

`m4::seat_from` turns that resolution into a `Seat { provider, model, model_alias, effort }`, and `m4::seat_for` reads the file first. `run_worker_inner` builds one seat before it inserts the task row and uses it for everything downstream.

### The model is part of the cache key, so the override has to reach `freeze`

This is the part that is easy to get subtly wrong. Prompt caching is keyed on **exact system-prompt bytes + tool set + model** ([02](02-context-engine.md)), and `studio_context::freeze` hashes all three into the `prefix_hash`. If the override were applied only to `WorkerSpec.model` — the thing that becomes `--model` on the command line — the studio would freeze against `role.model` and spawn against the user's model. The `prefix_hash` on `worker_spawned`, on `cache_hit`, and in the ledger would then name a prefix that no worker ever used, and the per-role `cache_hit_ratio` health metric would be measuring a fiction.

So the resolved model goes to `freeze` **and** to the spawn:

```rust
let seat = seat_for(role, &em.state.studio_dir);
let prefix = freeze(&charter, &tools, seat.model)?;
let spec = WorkerSpec { model: seat.model, effort: seat.effort, .. };
let args = spec.to_args_for(seat.provider, &seat.model_alias);
```

`the_overridden_model_is_the_one_the_prefix_is_frozen_against` pins this: it freezes the same charter twice, once against the registry model and once against the override, and asserts the two hashes differ. A future refactor that drops the override on the way to `freeze` fails there rather than in a cratered cache ratio six weeks later.

Two honest consequences of moving a seat:

- The new model mints a **fresh prefix**, so the first spawn after a change pays the 2.0× cold write premium. Moving seats around costs money; moving them back does too.
- `min_cacheable_tokens` differs by model (Fable 2048, Opus/Haiku 4096), so a seat moved from Fable to Opus is padded further before it caches at all.

### What the override does not reach

`m4::prefix_tokens_for(role, acting)` — the budget pre-estimate the workflow executor calls per node — still estimates against `role.model`. It takes no studio directory and is called from `wf.rs`, which this work does not own. The number it produces is a **budget estimate, not a cache identity**, and it is wrong only in the padding term for a seat the user has moved between Fable and Opus. Worth fixing when `wf.rs` next changes hands; not worth a signature change across an owner boundary now.

## Providers

`studio_core::Provider` is the program to run, how its arguments are built, and what it can actually do. `WorkerSpec::to_args()` still exists and is still claude: it is defined as `to_args_for(Provider::Claude, self.model.cli_alias())`, and `the_claude_command_line_is_frozen_byte_for_byte` asserts the full argument vector literally. Those bytes are the prompt-cache key; a test that only checked a few flags would let a reordering through, and a reordering throws away every warm prefix in the studio.

`Provider::brief_delivery()` records the one structural difference: claude takes the task brief on **stdin**, and the others take it as a `-p <text>` argument. `Worker::spawn_in` pipes stdin either way, so a prompt-argument provider is spawned with an empty stdin.

### Capability table

| | claude | gemini | copilot | kimi |
|---|---|---|---|---|
| program | `claude` | `gemini` | `copilot` | `kimi` |
| **flags read on a real install** | **yes** | **yes** | **yes** | **no** |
| frozen charter as a system prompt | `--system-prompt-file` | **no flag exists** | **no flag exists** | unknown |
| streamed events | `--output-format stream-json --include-partial-messages --verbose` | `--output-format stream-json` | `--output-format json --stream on` | unknown |
| token usage the studio can read | terminal `result.usage` | **not exposed** | **not exposed** | unknown |
| tool restriction | `--tools` | **no** — `--allowed-tools` only skips confirmation | `--available-tools` | unknown |
| structured output | `--json-schema` | **no** | **no** | unknown |
| session control | `--session-id`, `--resume`, `--fork-session` | `--resume` by index only | `--session-id`, `--resume` | unknown |
| effort | `--effort low…max` | none | `--effort none…max` | unknown |

**Verified against a real installed CLI:** claude, gemini and copilot. Each row above for those three was read out of `--help` on the machine this was built on — not from documentation and not from memory. What was **not** done for gemini and copilot is an actual spawn: no worker has been driven through either CLI, so the shape of their event streams is unconfirmed and the studio's stream parser is claude-shaped.

**Not verified at all:** kimi. It is not on PATH here, so its flags could not be read. The studio therefore refuses to spawn it rather than guessing a command line, and says so in those words. `a_cli_the_studio_never_probed_is_refused_before_any_flag_is_guessed` holds `to_args_for(Kimi, ..)` empty so there is nothing to accidentally execute.

### Why claude is currently the only provider that can serve a seat

`Provider::blockers(needs)` returns the reasons a provider cannot take a role, each ending in what to do instead. Three of them apply to every seat, not just some:

- **No system-prompt flag.** Neither gemini nor copilot has a flag that *replaces* the system prompt. The frozen L0+L1+L2 prefix is the entire token thesis ([02](02-context-engine.md)); delivering it in the user turn instead would work and would quietly cost 17.4× more per spawn. That is the degradation this design refuses to do silently, so it is refused loudly instead.
- **No readable token usage.** Budget governance ([06](06-budget-governance.md)) and the ledger ([03](03-state-store.md)) both read the numbers off the stream. Without them the studio would be spending blind and the floor would be showing numbers nobody measured.
- **No output schema** (gemini, copilot): the studio director's plan is read back as JSON against a schema. This one is per-role, and is reported separately as `plan_blockers` so the UI can say "this CLI could take the specialists but not the director" the day the first two are solved.

The arg builders for gemini and copilot are written and tested anyway. They are built from real flags, so the day one of those CLIs gains a system-prompt flag the change is one line in `capabilities()`, not a new integration.

`copilot`'s `--agent <agent>` and gemini's `GEMINI.md` are the nearest mechanisms to a replaced system prompt. Neither replaces it — both append to a default prompt, and neither was tested — so neither is wired up.

The panel lists every known provider with its capabilities and its blockers, and offers as selectable only the ones actually found on PATH.

## Subscription limits

**What the CLI actually exposes was checked before anything was built.** `claude --help` lists no usage or limits subcommand. `claude auth status --json` is real and returns `loggedIn`, `authMethod`, `email` and `subscriptionType`. The stream carries a real `rate_limit_event`, already parsed by `studio_core::stream` and present in the captured fixture:

```json
{"type":"rate_limit_event","rate_limit_info":{
  "status":"allowed","resetsAt":1784575200,"rateLimitType":"five_hour",
  "overageStatus":"rejected","isUsingOverage":false}}
```

So the studio knows, honestly: **which window is in force, whether it is still allowing work, and when it resets.** It does **not** know how much of the window is left — no percentage, no token count, no request count. `GET /limits` therefore returns:

- `account` — plan and signed-in address from `claude auth status --json`, cached 60s, with `source` naming the command it came from. When claude is not on PATH or nobody is signed in, `known: false` and the reason.
- `windows` — one entry per `rateLimitType` the daemon has observed, with `status`, `resets_at` and `observed_at`. There is no remaining field, because there is no remaining number. Only `five_hour` has ever been seen in a real stream; a weekly window would appear here the moment one arrives, keyed by whatever type the CLI names.
- `note` — says plainly either that no worker has reported a window yet, or that the CLI never says how much is left.
- `ledger` — the studio's own measured numbers from `cache_health`: cache reads against writes over the last 24 hours, the warm ratio, and how many distinct prefixes. `known: false` when nothing has been billed rather than a zero that reads like a measurement.

`m4`'s drive loop calls `studio_server::settings::observe_rate_limit` on every `CliEvent::RateLimit`, which keeps the latest window per type. It is process-local and deliberately not persisted: a reset time read from a stream three days ago is not a fact about now.

The panel shows "windows unavailable" until a worker has run. It never renders a percentage or a countdown the CLI did not give it.

## Music

About twenty files in a folder, playing on the floor.

| Route | Does |
|---|---|
| `GET /music` | `{folder, exists, tracks:[{name, bytes}], playable}` |
| `GET /music/track?name=…` | one file, correct content type, `Accept-Ranges: bytes`, `206` with `Content-Range` when a range is asked for |

The folder defaults to `<studio_dir>/music` and is changed with `music.folder`, chosen through the existing `/fs/browse` picker. `exists: false` is reported distinctly from an empty list, because "you have not made that folder yet" and "that folder has nothing playable in it" need different fixes. Only extensions a browser will actually decode are listed.

`name` must be a plain file name: no separators, no `..`, no drive colon. A path that climbs out of the music folder is a `400` before the filesystem is touched.

Two front-end rules the browser forces:

- The `<audio>` element lives on `document.body`, not inside the panel div, so switching tabs does not tear playback down mid-track.
- Nothing plays without a user gesture. Music is disabled by default, and playback is only ever started from a click — ticking the enable box, pressing play, pressing next, or the `ended` handler of a track the user already started. On page load a previously enabled setting restores the selection but does not start audio; a blocked `play()` says so instead of failing silently.

## Low spec

`lowSpec` is a plain boolean, described in the panel as dropping the heavy parts of the 3D floor so an older machine keeps a steady frame rate. The scene consumes it ([12](12-visual-workspace.md)); this document only owns the key.
