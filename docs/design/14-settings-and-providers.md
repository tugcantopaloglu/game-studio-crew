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
| `models.tier1` `.tier2` `.tier3` | `fable` \| `opus` \| `sonnet` \| `haiku` | the model for every seat in that tier |
| `models.role.<role_id>` | same | that one seat's model |
| `models.<provider>.tier<N>`, `models.<provider>.role.<id>` | free text | the model name handed to a non-claude CLI |
| `effort.tier<N>`, `effort.role.<id>` | `low` … `max` | `--effort` for those seats |
| `limits.enabled` | bool | whether the panel polls `/limits` |
| `limits.refreshSeconds` | 1800 \| 300 \| 60 | how often |
| `music.enabled` `.folder` `.track` `.volume` `.shuffle` | | the floor's music |
| `lowSpec` | bool | consumed by the 3D scene |
| `motion.crew` | `auto` \| `on` \| `off` | whether the crew walks the floor |
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

| | claude | codex | gemini | copilot | kimi |
|---|---|---|---|---|---|
| program | `claude` | `codex` | `gemini` | `copilot` | `kimi` |
| **flags read on a real install** | **yes** | **yes** | **yes** | **yes** | **no** |
| frozen charter as a system prompt | `--system-prompt-file` | **no flag exists** (AGENTS.md only) | **no flag exists** | **no flag exists** | unknown |
| streamed events | `--output-format stream-json --include-partial-messages --verbose` | `exec --json` | `--output-format stream-json` | `--output-format json --stream on` | unknown |
| token usage the studio can read | terminal `result.usage` | **prose on stderr only** | **not exposed** | **not exposed** | unknown |
| tool restriction | `--tools` | **no** — `--sandbox` limits writes, not tools | **no** — `--allowed-tools` only skips confirmation | `--available-tools` | unknown |
| structured output | `--json-schema` | `exec --output-schema` | **no** | **no** | unknown |
| session control | `--session-id`, `--resume`, `--fork-session` | `resume`, `fork` | `--resume` by index only | `--session-id`, `--resume` | unknown |
| effort | `--effort low…max` | `-c model_reasoning_effort=` | none | `--effort none…max` | unknown |
| brief arrives as | stdin | positional argument | `-p` | `-p` | unknown |

**Verified against a real installed CLI:** claude, codex, gemini and copilot. Every row above for those four was read out of `--help` on the machine this was built on — not from documentation and not from memory. codex went further: its `exec` command line was actually run, both to a working model and to a refused one, and the stream split in the table above is measured.

What was **not** done for gemini and copilot is an actual spawn: no worker or probe has been driven through either, so the shape of their event streams is unconfirmed and the studio's stream parser is claude-shaped.

**Not verified at all:** kimi. It is not on PATH here, so its flags could not be read. The studio therefore refuses to spawn it rather than guessing a command line, and says so in those words. `a_cli_the_studio_never_probed_is_refused_before_any_flag_is_guessed` holds `to_args_for(Kimi, ..)` empty so there is nothing to accidentally execute.

### Why claude is currently the only provider that can serve a seat

`Provider::blockers(needs)` returns the reasons a provider cannot take a role, each ending in what to do instead. Three of them apply to every seat, not just some:

- **No system-prompt flag.** Neither gemini nor copilot has a flag that *replaces* the system prompt. The frozen L0+L1+L2 prefix is the entire token thesis ([02](02-context-engine.md)); delivering it in the user turn instead would work and would quietly cost 17.4× more per spawn. That is the degradation this design refuses to do silently, so it is refused loudly instead.
- **No readable token usage.** Budget governance ([06](06-budget-governance.md)) and the ledger ([03](03-state-store.md)) both read the numbers off the stream. Without them the studio would be spending blind and the floor would be showing numbers nobody measured.
- **No tool restriction** (gemini): every spawn passes an explicit `--tools`, including the coordination seats whose list is deliberately **empty**. An empty allowlist is the strongest restriction the studio applies, not the absence of one — it is the difference between 22572 and 184 prefix tokens ([02](02-context-engine.md)) — so `restricted_tools` is asked of every seat, never inferred from whether the list has entries in it.
- **No output schema** (gemini, copilot): the studio director's plan is read back as JSON against a schema. This one is per-role, and is reported separately as `plan_blockers` so the UI can say "this CLI could take the specialists but not the director" the day the others are solved.

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

## Which models are actually usable

Every model list in the studio used to be hardcoded, which rots quietly and then lies: this machine's `~/.codex/config.toml` pins `model = "gpt-5.2-codex"`, a name the account refuses, and nothing noticed.

**No CLI here has a subcommand that lists models.** Checked, not assumed: `claude --help` offers agents, auth, auto-mode, doctor, gateway, install, mcp, plugin, project, setup-token, ultrareview and update; `codex --help` offers exec, review, login, logout, mcp, plugin, mcp-server, app-server, remote-control, app, completion, update, doctor, sandbox, debug, apply, resume, archive, delete, unarchive, fork, cloud, exec-server and features. Neither has a catalogue call. There is no cheap way to ask, so the studio asks the expensive way, and only when told to.

### `GET /models`

Per provider: the offered candidates, where each name came from, and what happened when it was last checked.

| Source | Means |
|---|---|
| `cli_help` | named in that CLI's own `--help` output |
| `picker` | listed by the CLI's own interactive model picker |
| `user_config` | found in this machine's config file for that CLI |
| `settings` | you typed it into the studio settings |
| `probe` | the studio has probed this name before |

A name can carry several sources at once, and they are all reported. `gpt-5.6-sol` is both a `picker` entry and a `user_config` one on this machine, because codex's own `[tui.model_availability_nux]` table records having shown it.

Verdicts are exactly three: **`working`** (it answered), **`refused`** (it did not, with the CLI's own words), **`unknown`** (nobody has checked). A model that has never been probed reads as `unknown` and is still offered — never hidden for being unprobed, and never dressed up as working. `a_model_nobody_has_checked_reads_as_unknown_rather_than_as_working` walks every candidate of every provider on a fresh studio and asserts it.

### Probing is asked for, never automatic

`POST /models/probe` with `{"provider": "codex", "models": ["gpt-5.6-luna"]}`. It is a real billed request per model and takes real seconds, so:

- **Nothing probes on panel open.** `GET /models` never spawns anything. Posting an empty list is a `400` whose message says why, and more than twelve at once is refused too.
- The panel states the cost — one real request per model on that CLI's own subscription, up to three minutes each — before the button does anything.
- Results cache to `.studio/model-probes.json` with a timestamp, and **a cached refusal is kept exactly as carefully as a cached success**. Reading a corrupt or absent cache yields "nothing checked yet", never a guess.
- Each probe is bounded and its process tree killed on timeout, so a wedged CLI cannot hold the route open.

### How a probe decides, and the two traps in it

The question is `what is 17 plus 25? reply with just the number` and the answer is `42`.

**Trap one: codex echoes the prompt.** Probing with "reply with pong" and grepping for `pong` matches your own request, and every model grades as working. The question is therefore arithmetic whose answer cannot appear in it — `the_probe_question_cannot_contain_its_own_answer` asserts that literally — and `probe_answered` strips occurrences of the question before looking for the answer, so an echo alone can never pass.

**Trap two: stderr is full of unrelated noise.** This machine's codex config registers a dead MCP server at `127.0.0.1:8080`, so every codex run logs `HTTP 404 Cannot POST /mcp`. A probe therefore never judges by exit code or by stderr being empty — only by whether the answer is present.

Measured on this machine, streams separated:

| | stdout | stderr |
|---|---|---|
| `codex exec -m gpt-5.6-luna` (works) | `42` and nothing else, 3 bytes | banner, echoed prompt, `tokens used` / `1,668` |
| `codex exec -m gpt-5.2-codex` (refused) | **empty, 0 bytes** | MCP 404 noise, then `The 'gpt-5.2-codex' model is not supported when using Codex with a ChatGPT account.` |

Two things fall out of that table and both were bugs first. The token count is on **stderr**, so reading stdout alone silently loses it. And the refusal reason is on stderr too, mixed in with the MCP noise, so `explain_refusal` filters known noise and then ranks the remaining lines strongest-signal-first — because taking the first plausible line picked `warning: Model metadata for gpt-5.2-codex not found`, a fallback-metadata warning, over the actual reason. `a_refusal_is_explained_in_the_cli_s_own_words_not_by_a_dead_mcp_server` pins all three failures at once.

`claude` refuses an unknown model for **$0**: `--output-format json` returns `is_error: true`, `api_error_status: 404`, `total_cost_usd: 0` and a `result` of "There's an issue with the selected model (X). It may not exist or you may not have access to it." Checking a claude name that does not exist is free; checking one that does costs one small call, and the probe records the `total_cost_usd` the CLI itself reported rather than an estimate.

### Where the list is surfaced

Every place a model is chosen: the three tier pickers and all thirteen per-role pickers. Each is a **free-text input with the discovered list as `<datalist>` suggestions**, not a closed dropdown, so a model released next month can be typed in today; underneath each one sits the verdict for the name currently in it. The catalogue is also exported from `settings.js` as `models(providerId)` and reachable at `GET /models`, so no other panel needs a list of its own.

### Sonnet, and what widening the model enum actually cost

`studio_context::Model` carried `Fable`, `Opus` and `Haiku` while the claude CLI's own `--help` also names **`sonnet`** — the studio could not express a model the CLI accepts. `Model::Sonnet` now exists, at $3/$15 per MTok.

**Adding a variant cannot move an existing model's prefix hash, and the reason is worth stating precisely.** `freeze` hashes the model as `model.cli_alias()` — the *alias bytes*, `b"opus"`, not the enum discriminant and not its `Debug` form:

```rust
hasher.update(b"\x00model\x00");
hasher.update(model.cli_alias().as_bytes());
```

So variant *position* is not load-bearing and a new variant anywhere in the enum is safe. Had the hash been built from the discriminant, inserting `Sonnet` between `Opus` and `Haiku` would have silently re-keyed Haiku's every cached prefix. Two tests pin this rather than trusting it: `the_hash_is_built_from_the_cli_alias_not_from_the_enum_position` recomputes the digest from the alias bytes and asserts it matches, and `adding_a_model_never_moves_an_existing_models_prefix_hash` asserts the literal blake3 hashes of a fixed charter for fable, opus and haiku. A future refactor to discriminant-based hashing fails both.

The registry is untouched: **sonnet is available to choose, not assigned to anything.** `widening_the_enum_moved_no_role_off_the_model_it_ships_on` asserts no role adopts it and that `studio_director` is still the only Fable seat.

#### The padding number, and a discrepancy worth someone's attention

`min_cacheable_tokens` is the one part of a new variant that can silently break: pad below a model's minimum cacheable prefix and its charters **never cache at all** — `cache_creation` stays 0 with no error ([02](02-context-engine.md)).

Sonnet's minimum is **documented as 1024 tokens**. The studio pads it to **4096** anyway, matching what Opus and Haiku already use. That is deliberate: padding *above* a minimum still caches, padding below it caches nothing, and a coherent padding scale is worth more than 3k tokens paid once per prefix. `documented_min_cacheable_tokens` records the published figure separately from the padding the studio applies, and `every_model_pads_at_or_above_the_minimum_its_documentation_states` asserts the invariant for every variant.

#### Measured: the 640 target caches, and the estimator errs the safe way

The padding was subsequently derived from the published floor (`documented + documented/4`), which puts an Opus charter at 640 estimated tokens. That target was **measured against a real spawn** rather than reasoned about, because "silently never caches" is invisible by construction.

A `gameplay_engineer` charter padded to the 640 target is 2333 characters and estimates at 649 tokens. Spawned twice with `--tools ""` against `--model opus`:

| | `input` | `cache_creation` | `cache_read` | cost |
|---|---|---|---|---|
| cold | 2 | **926** | 0 | $0.0133 |
| warm | 2 | 0 | **926** | $0.0015 |

**It caches**, and the warm spawn is 8.7× cheaper. So 640 is not too tight: 926 real tokens against Opus 5's 512-token floor is 1.81× headroom.

The interesting part is *why* it is safe, because it is not the reason the margin was added. The estimator assumes **3.6 characters per token**; this charter measured **2.52** (2333 chars ÷ 926 tokens). The estimator therefore reads **low, not high** — it called 649 what the tokenizer counted as 926, a 1.43× undercount. Charter text is dense: short words, heavy punctuation, newlines, and numbered padding lines whose digits cost a token apiece.

That direction matters more than the specific number. The quarter margin is harmless, but it was justified as protection against the estimator over-counting a prefix to just *under* the floor, and the error runs the other way: `estimate_tokens` is already a conservative lower bound on real tokens, so every model over-delivers against its floor before the margin is applied at all. Anyone reasoning from "the estimator reads high" will get the next call wrong in the opposite direction — trimming the margin as though it were slack, or treating an estimate as an upper bound when padding something shorter.

Two consequences worth carrying: **Haiku now has the slack**, not Opus — a 5120 estimated target is roughly 7300 real tokens against a 4096 floor, about 78% over, and it can be computed from the measured 1.43 ratio rather than guessed. And **the failure mode remains silent**: a charter padded below its floor produces `cache_creation: 0` *and* `cache_read: 0`, which reads as "no data" rather than "broken", so it would not surface as a cratered `cache_hit_ratio` the way a churning prefix hash does. A cold spawn that writes zero cache bytes is a detectable signal the ledger already has and nothing currently asserts.

#### The over-padding this separation originally exposed

Keeping the two numbers apart surfaced something this document should not bury: **the studio's padding table was well above the published minimums** (since corrected — the table below is the state that prompted it).

| | studio pads to | published minimum |
|---|---|---|
| fable | 2048 | 512 |
| opus | 4096 | 1024 |
| sonnet | 4096 | 1024 |
| haiku | 4096 | 4096 |

If those published figures are right, every Opus seat — which is twelve of the thirteen roles — pads roughly 4× further than it needs to, and the studio pays that padding on every cold write. **This was not changed, and should not be changed casually:** `min_cacheable_tokens` feeds `pad_to_minimum`, which changes the frozen bytes, which changes the `prefix_hash`. Lowering Opus from 4096 would invalidate every warm prefix in the wild in exchange for a saving nobody here has measured. It is worth a probe — freeze a short charter at each candidate padding and read `cache_creation` off a real spawn — before it is worth an edit. `the_padding_the_studio_uses_is_not_mistaken_for_the_published_minimum` pins which models currently over-pad so the question cannot quietly disappear.

#### Sonnet does not become the summarizer downshift target

`studio_budget::model_for_step` sends the summarizer to Haiku on ladder step 2 ([06](06-budget-governance.md)). Sonnet sits between Opus and Haiku on price, so it is a plausible candidate — and it is **deliberately not used**.

The step's entire purpose is to cut spend when a task is already over budget. Haiku at $1/$5 saves roughly 80% against Opus; Sonnet at $3/$15 saves roughly 40%. Choosing the middle tier would make the cost-saving step save materially less, which is the opposite of what the ladder is for. If a rollup genuinely needs more capability than Haiku, the right answer is not a more expensive downshift — it is that the task should not be downshifted at all.

The quality objection is already answered elsewhere: the summarization ladder has a **zero-token template fallback that always works** ([02](02-context-engine.md)), so an unusable rollup degrades to deterministic counts and concatenated fields rather than needing a better model. And nobody here has measured Sonnet against Haiku on rollup quality — swapping on that basis would be substituting a guess for a documented, measured-cheap choice.

`the_summarizer_downshifts_to_the_cheapest_tier_not_the_middle_one` records the decision and its reasoning at the point of change, so a future reader finds the argument rather than re-deriving it. If someone does measure the two, that test is where the finding belongs.

#### A model the studio cannot express is still refused

Unrecognised claude model names are **refused at spawn with the name in the message** rather than quietly falling back to the registry. Full names resolve by family (`claude-opus-5` → `Opus`, `claude-sonnet-…` → `Sonnet`), and anything else is refused with a message listing what does work. `a_claude_model_the_studio_cannot_express_is_flagged_rather_than_quietly_replaced` holds that line, and `every_model_the_studio_can_express_actually_resolves` checks the advertised list is not lying.

### A refused model is never a generic failure

When a spawn fails, the message names the provider and the model — `Seat::describe()` renders `codex gpt-5.2-codex` — and quotes the CLI's own terminal message rather than the first line of streamed prose. So a refusal reads as "artist on codex gpt-5.2-codex failed: The 'gpt-5.2-codex' model is not supported…", not "artist failed".

## Low spec

`lowSpec` is a plain boolean, described in the panel as dropping the heavy parts of the 3D floor so an older machine keeps a steady frame rate. The scene consumes it ([12](12-visual-workspace.md)); this document only owns the key.

## Crew motion

`motion.crew` is three-valued rather than a boolean, because the interesting state is *not having chosen*. On `auto`, the default, the floor follows `prefers-reduced-motion` and parks the crew at their desks whenever the operating system asks for reduced motion. That is the right default and the wrong lock: Windows reports reduced motion whenever "animation effects" is off, which is a display preference many machines have off without anyone having asked for a still office. `on` and `off` are the explicit answers, and once one is stored the system preference stops being consulted. `perf.js` owns the resolution and publishes it through `onMotion`, so the floor picks up a change without a reload.
