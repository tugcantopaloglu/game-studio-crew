# 19: Asset Generation

> **Status:** v0.1, 2026-07-25. Built: the capability probe, the settings key, the engine-aware destination plan, the codex invocation, post-generation verification through the existing model export bridge, the project manifest, the `codex-assets` project skill, and the assets panel.
> **This document is the single source of truth for:** what `codex` can and cannot be asked for, where a generated asset lands per engine, and the terms on which the whole feature is absent. [07](07-engine-layer.md) owns the engine profiles this reads; [14](14-settings-and-providers.md) owns the settings file this adds one key to.

## What codex can actually do, measured

This was checked before anything was designed, because the obvious reading of "generate a character with codex" is wrong.

`codex-cli 0.145.0`, on the machine this was built on:

| Asked | Answer |
|---|---|
| Is there an image-generation subcommand? | **No.** The full command list is exec, review, login, logout, mcp, plugin, mcp-server, app-server, remote-control, app, completion, update, doctor, sandbox, debug, apply, resume, archive, delete, unarchive, fork, cloud, exec-server, features, help. |
| What is `-i/--image` then? | **Input only.** "Optional image(s) to attach to the initial prompt", on both `codex` and `codex exec`. |
| Does a plugin add it? | **No.** `codex plugin list` reports no marketplace plugins found. |
| Does an MCP server add it? | **No.** `codex mcp list` has exactly one entry, `unityMCP` at `http://127.0.0.1:8080/mcp`, and it was not even answering (`HTTP 404 Cannot POST /mcp`). |
| Is there a feature flag for it? | **No.** Nothing in `codex features`. |

So **codex cannot produce a raster image.** It reads images and writes code. That is the whole shape of this feature, and it is the same shape the studio already uses for art: [07](07-engine-layer.md) tells the `web` and `godot` crews to build any model that comes from a reference image with the img2threejs skill, as a procedural three.js factory. Reference image in, procedural asset code out. This document adds a second producer of exactly that artefact, driven by a CLI rather than by a worker.

Anything in the UI or in a charter that implied codex draws pictures would be a lie the studio told its user, so `GET /assets` carries the sentence "codex cannot draw a picture" and the panel prints it.

### Four more measurements that changed the design

**`codex exec --sandbox workspace-write` reported `sandbox: read-only` in its own session header on this Windows box.** So codex is not asked to write files. It is asked for the source text through `--output-schema`, and **the daemon writes the file**. That is better anyway: the destination is the one the engine profile dictates, not one the model chose, and a generation that fails cannot have left a half-written file in the project.

**`codex exec` hangs forever if its stdin is left open.** The first real run died at the ten-minute cap with `Reading additional input from stdin...` and nothing else in the log: given a prompt argument *and* an open pipe, codex waits for EOF on the pipe before it starts. `run_codex` therefore sets `Stdio::null()`. This is the kind of thing [ADR 0004](adr/0004-explicit-context-control-not-bare.md) is about — the flag documentation says stdin is "appended as a `<stdin>` block" and does not mention that an unclosed pipe is a deadlock.

**codex authenticates per machine and its token expires.** When the refresh token is spent, every request fails with `401 token_expired` and the fix is an interactive `codex login`, which no daemon can do. That is a blocker to report, not an error to raise, which is why a failed generation degrades.

**The model in a user's `~/.codex/config.toml` can be one their account cannot use.** On this machine the configured default `gpt-5.2-codex` returned `400`: "not supported when using Codex with a ChatGPT account", and so did `gpt-5-codex`, `gpt-5.1-codex`, `gpt-5.2` and `codex-mini-latest`. `codex debug models` listed what was actually available: `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`. No model name is hard-coded here, because that list is exactly the sort of thing that rots; `diagnose` turns that specific `400` into a sentence naming `codex debug models` and the `assets.model` setting.

## Optional on the studio's existing terms

Two mechanisms, both already in the repo, and no third one:

- **The capability table.** `assets::blockers(installed, enabled, engine)` returns a list of reasons, each ending in what to do next, exactly like `studio_core::Provider::blockers(needs)`. `Capability::ready()` is "the list is empty". Nothing infers availability from whether a binary happens to exist.
- **The per-project optional skill.** `skills::ensure_codex_assets` writes `.claude/skills/codex-assets/SKILL.md` into the project the same way `ensure_img2threejs` clones one in, returns `true`/`false`/`Err` and never fails a run.

The three things that can block it:

| Blocker | Said when |
|---|---|
| switched off | `assets.enabled` is not `true`. **This is the default.** |
| `codex` not on PATH | `on_path("codex")` finds nothing. Names the install command and says the art crew keeps working by hand. |
| the engine has nowhere to put a model | the project is `python`, or no engine was detected. |

**With the feature off, or with codex absent, every existing path behaves exactly as it does today.** `crew_hint` returns the empty string, so not one byte is added to any charter or brief and no prefix hash moves ([02](02-context-engine.md)). No skill is installed. No file is written into the project. `a_studio_that_never_turned_this_on_hands_the_crew_no_extra_words_at_all` pins the empty string, because a hint that leaked in when the feature was off would silently cold-start the prompt cache for every art seat.

### The one settings key

| Key | Value | Default | Changes |
|---|---|---|---|
| `assets.enabled` | bool | **`false`** | whether the crew may spend Codex budget on assets |
| `assets.model` | free text | unset | the `-m` handed to codex; unset means the codex default |

Flat keys in `.studio/settings.json`, read through `studio_settings::Settings`, written by the panel through the existing `POST /settings` merge. Nothing new persists anywhere else.

## Where a generated asset lands

Not invented here. `plan_for(engine, slug)` reproduces what the engine profiles and the `engine_hint` prose in `wf.rs` already tell workers:

| Engine | factory | then |
|---|---|---|
| `web` | `src/models/<slug>.js` | the browser imports it directly; no export step |
| `godot`, `unity`, `ue5` | `tools/models/<slug>.mjs` | baked to `assets/models/<slug>.glb`, which the engine imports |
| `python`, undetected | — | refused with a reason |

The factory contract is the one `tools/model_export.mjs` already enforces, because that helper is the verifier: **one default export, taking `THREE` as its only argument, returning a `THREE.Group`, importing nothing.** A factory that imported `three` would be unloadable in node and so unverifiable, which is why `looks_like_a_factory` rejects an import before the file is written rather than after.

For the `.glb` engines the prompt also says textures, canvases and data URIs do not survive the export. That is not a guess; it is what the `godot` profile in [07](07-engine-layer.md) already tells the art crew.

## The generation, step by step

```
codex exec --skip-git-repo-check --sandbox read-only --color never \
  -C <project> --output-schema <schema.json> -o <answer.json> \
  [-m <assets.model>] [-i <reference image>] "<the brief>"
```

1. **Refuse first.** `Capability::ready()`, then a name that slugifies to something, then a non-empty description. An asset with no description is refused before codex is paid to guess.
2. **Ask.** The answer is `{source, notes}` against `ANSWER_SCHEMA`. stdout and stderr go to `.studio-out/assets/<slug>.codex.log`, a file rather than a pipe, so a chatty run cannot deadlock on a full pipe buffer. The wait is bounded at ten minutes and the child is killed on overrun.
3. **Read the source, not the filesystem.** `parse_answer` refuses anything that is not the schema and quotes the first 200 characters of what arrived instead, because "codex said something else" and "codex said nothing" need different fixes.
4. **Check what can be checked cheaply.** `looks_like_a_factory` rejects a missing `export default`, any `import`, and any `require()`.
5. **Write it, then prove it loads.** `node tools/model_export.mjs <factory> <proof.glb>`. A non-zero exit, an unreadable report, or **a mesh count of zero** is a failure. Zero meshes matters: a factory that returns an empty group exports cleanly and renders nothing.
6. **Degrade, never fail.** On any failure the factory file is **restored to what it was, or deleted if it did not exist**, and the reason names the log. The run continues; nothing raises.

`a_generation_that_cannot_run_leaves_the_project_exactly_as_it_found_it` asserts the refusal path writes neither `src/` nor `.studio-out/`. `generating_while_the_feature_is_off_refuses_without_spending_anything` drives the real route and asserts `200` with `ok: false` — a refusal is an answer, not a server error, because a 500 would make the floor look broken when the studio is merely switched off.

Successes are appended to `.studio/assets.json` in the project, keyed by slug so regenerating replaces a row instead of doubling it. That is the same place and shape as `.studio/game.json` ([games](../../crates/studio-server/src/games.rs)).

## What the crew gets

When the feature is ready, `announce` installs the `codex-assets` skill and `crew_hint` adds one paragraph naming it plus **the assets already generated and their paths**, so the second art task loads the scout rather than sculpting a new one.

The skill body is composed from the same `AssetKind::shape()` strings and the same `plan_for` result the daemon uses, so the instructions a worker reads and the prompt the daemon sends cannot drift apart. The art and audio seats already carry `Bash` and `Skill` in their tool allowlist ([04](04-agent-graph.md)), so a worker can drive codex itself; the skill tells it to ask for source and write the file itself, for the same sandbox reason the daemon does.

## Two asset classes

| Kind | Asked for |
|---|---|
| `character` | reads as a rig: named head, torso, arm and leg parts about sensible joints, ~1.7 units tall, lowest point at `y = 0`, facing `-Z` |
| `prop` | one static object, no limbs, no implied joints, centred on x and z, resting at `y = 0`, under 40 meshes |

Both land in the same place, because that is what the engines consume. The difference is entirely in what codex is asked for, and it is a real difference: a prop asked for with the character prose comes back with legs.

`tileset` and `sprite sheet` are deliberately absent. Both would need a raster image or a new project convention, and this studio has neither: `AssetKind::from_key("tileset")` returns `None` and the route refuses it by name.

## Routes

| Route | Does |
|---|---|
| `GET /assets[?project=<id>]` | capability, blockers, the two kinds and their prose, where a file would land, and the assets already generated |
| `POST /assets/generate` | `{project, kind, name, description, reference?}`; `200` with `ok:true` and the record, or `200` with `ok:false` and a reason |

A kind the studio does not make is a `400` that lists the kinds it does.

### A reference image is the one untrusted path in this feature

Everything else the route takes is either a store-resolved project id or text that gets slugified. `reference` is different: it names a file, and that file is **uploaded to OpenAI** as part of the prompt. A bare join against the project root would have let `../../../.ssh/id_rsa` be attached to a request, so `reference_in` refuses in four ways before the filesystem is trusted:

- the extension must be one of `png`, `jpg`, `jpeg`, `webp`, `gif`
- the path may not be absolute, contain `..`, or contain `:`
- it is then **canonicalized and checked to still be inside the canonicalized project root**, which is what catches a symlink pointing out
- and only then must it exist as a file

`a_reference_that_climbs_out_of_the_project_is_never_uploaded_to_codex` drives the escaping cases against a real file planted outside the project. The string checks alone would not be enough — a symlink passes all of them — which is why the containment check is on the resolved path rather than the given one. This mirrors `is_a_plain_file_name` on the music route ([14](14-settings-and-providers.md)), for the same reason: a path from a browser is an input, not a fact.

## What is verified, and what is not

| Claim | How it is known |
|---|---|
| codex has no image generation | **read off the installed CLI**: `--help`, `exec --help`, `plugin list`, `mcp list`, `features` |
| `--sandbox workspace-write` still reports read-only here | **observed** in a real `codex exec` session header |
| the feature is off by default, reports its blockers, and degrades | **unit tested** |
| a generated factory lands where the engine expects it | **unit tested** against `plan_for` |
| the export bridge report is parsed for the mesh count | **unit tested** against a real report line |
| a character is generated end to end and loads | **done for real, once**; see below |
| a prop is generated end to end | **not done.** Only the prompt and the destination are tested. |

The panel has **not** been seen in a browser. There was no connected Chrome on the machine this was built on, so `assets.js` is checked by `node --check`, by the module and panel-host tests in `web.rs`, and by a run under the `probes/floor-dom.mjs` stubbed DOM which mounts it against a stubbed `/assets` and asserts it hides the form when off, offers it when on, lists a previously generated asset, and reports a failed read instead of swallowing it. Nothing is claimed about how it looks.

### The one real generation

A `character` named **Scrapyard Scout**, on a scratch web project in a temp directory — not in this repo and not in the user's project store.

| | |
|---|---|
| model | `gpt-5.6-sol`, reasoning effort high (codex's own default) |
| wall clock | 2 minutes 2 seconds |
| cost | **unknown.** A ChatGPT-plan session reports no token count and no figure, so there is none to quote. |
| source | 10378 bytes, written to `src/models/scrapyard_scout.js` |
| verified by | `node tools/model_export.mjs src/models/scrapyard_scout.js .studio-out/assets/scrapyard_scout.glb` |
| which said | `wrote .studio-out/assets/scrapyard_scout.glb (186008 bytes, 54 mesh(es))` |

Loaded and measured afterwards: root group named `scrapyard_scout`, **69 named children** (`torso`, `patched_coat_body`, `coat_left_hem`, `head`, `hood`, `goggle_left_lens`, `satchel_crossbody_strap`, and so on), height **1.706** units against the 1.7 asked for, no imports, no comments anywhere in the file, one default export.

One miss worth recording: the lowest point came out at **y = -0.0061**, so it stands 6mm below the ground plane rather than exactly on it. The contract asked for `y = 0` and got within a centimetre. `verify` does not check this, because `model_export.mjs` reports only bytes and a mesh count and that helper belongs to [07](07-engine-layer.md); a bounding-box assertion is the obvious next thing to add the next time that file changes hands.

`Provider::Codex` in `studio_core` is a different thing to this and deliberately not reused: that is about spawning a *worker* as codex, which the provider table refuses because codex has no flag that replaces a system prompt. This feature invokes codex as a one-shot tool for a single asset, where no frozen charter is involved at all.
