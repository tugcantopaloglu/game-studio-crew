# 19: Asset Generation

> **Status:** v0.2, 2026-07-27. Built: the capability probe, the settings keys, the engine-aware destination plan, the codex invocation, raster generation through codex's built-in image tool, chroma-key background removal and its validation, the concept-art-to-model pipeline, post-generation verification through the existing model export bridge, the project manifest, the `codex-assets` project skill, the assets panel and the route that serves a generated image back to it.
> **This document is the single source of truth for:** what `codex` can and cannot be asked for, where a generated asset lands per engine, and the terms on which the whole feature is absent. [07](07-engine-layer.md) owns the engine profiles this reads; [14](14-settings-and-providers.md) owns the settings file this adds keys to.

## What codex can actually do, measured

**v0.1 of this document got this wrong, in the most expensive way a measurement can be wrong: it measured the wrong surface and then designed around the answer.** It reported that codex cannot produce a raster image, the route said so to every caller, and the skill taught it to every worker. Codex had been able to draw the whole time.

Here is what v0.1 asked, and what was actually true, on `codex-cli 0.145.0`:

| Asked | v0.1 answer | Actually |
|---|---|---|
| Is there an image-generation subcommand? | No | **Correct, and irrelevant.** Image generation is a tool the *model* calls, not a subcommand. Nothing about it can appear in `--help`. |
| What is `-i/--image` then? | Input only | **Correct.** It attaches an image to the prompt; it is not how one comes back. |
| Does a plugin add it? | No | **Correct, and irrelevant.** It is built in, not a plugin. |
| Is there a feature flag for it? | **No. "Nothing in `codex features`."** | **Wrong.** `codex features list` reports `image_generation  stable  true`. Bare `codex features` prints its own usage text and no flags at all, so the probe that produced this answer never saw the list it claimed to have read. |

The lesson is worth more than the feature: **a CLI's `--help` surface is not the boundary of what its agent can do**, and a command that prints usage instead of output has not answered the question. This is the same distinction [14](14-settings-and-providers.md) draws between "installed" and "drivable", one level up: *reachable through the CLI* and *listed by the CLI* are different facts.

So, measured properly and then run for real:

- codex draws raster images with a **built-in `image_gen` tool**, driven by the `imagegen` skill it ships at `$CODEX_HOME/skills/.system/imagegen/`.
- It needs **no `OPENAI_API_KEY`**: it goes through the same sign-in `codex login` already made. There are still no API keys anywhere in this studio.
- It works **non-interactively**, under `codex exec --sandbox read-only`, with `--output-schema`, which is the exact invocation this feature already used for source.
- The image lands at `$CODEX_HOME/generated_images/<session>/<call-id>.png` — **outside the project**, which is what lets the daemon keep owning every byte that reaches disk.

The one thing codex still cannot do is produce a transparent background directly: its built-in tool exposes no such control. So the studio asks for a flat chroma-key background and removes it locally with the remover codex ships beside the skill, `remove_chroma_key.py`.

Both halves are now used together, and that composition is the point of this feature: **codex draws the asset, the studio cuts its background off, and codex is handed its own drawing back as the reference it builds the procedural model from.** Reference image in, procedural asset code out is still the shape [07](07-engine-layer.md) describes for the img2threejs skill — the difference is that the studio no longer has to wait for a human to supply the reference.

## codex is a pure generator; the daemon owns every byte that reaches disk

This is the load-bearing rule of the feature, and it is not a workaround. It is the same invariant the studio already holds everywhere else: **no worker runs git, and no worker writes outside what the daemon sanctions** ([03](03-state-store.md), [10](10-standards-and-trust.md)). Commits are daemon-side for exactly this reason. Asset generation is the same shape — codex is handed a brief and returns text, and the daemon decides whether that text becomes a file, which file, and where.

Three consequences, each of them tested:

1. **The destination is derived from the engine profile, never from the model.** `plan_for(engine, slug)` produces it; codex is told the path only so its prose can be accurate about the export step. Nothing codex returns can redirect the write.
2. **The path is validated to stay inside the project root.** `inside(project, relative)` accepts only `Component::Normal` parts, so an absolute path, a `..`, or a drive prefix is refused before any write. The slug feeding it is already `[a-z0-9_]` only, so this is belt and braces — and `a_slug_cannot_be_crafted_to_write_outside_the_models_directory` asserts both halves, because the day someone loosens `slugify` the containment check is what still holds.
3. **A generated factory never replaces an existing file unless the caller asked.** `Request::overwrite` defaults to `false` and the panel surfaces it as an explicit "replace the file if one is already there" tick. The refusal happens **before codex is invoked**, so a collision costs nothing. `a_generated_asset_never_replaces_hand_written_art_unless_it_was_asked_to` plants a hand-written factory, asks for the same slug, and asserts the file is byte-identical afterwards and that no working directory was even created. A generator that can silently clobber hand-made art is worse than no generator.

### Four measurements that shaped this

**`codex exec --sandbox workspace-write` reported `sandbox: read-only` in its own session header on this Windows box.** Recorded here because the next person will otherwise assume that flag can be relied on: it cannot, at least not here. It is *not* the reason for the rule above — the rule would stand on a machine where the sandbox honoured the flag — but it does mean that asking codex to write the file would have failed anyway.

**`codex exec` hangs forever if its stdin is left open.** The first real run died at the ten-minute cap with `Reading additional input from stdin...` and nothing else in the log: given a prompt argument *and* an open pipe, codex waits for EOF on the pipe before it starts. This is the kind of thing [ADR 0004](adr/0004-explicit-context-control-not-bare.md) is about — the flag documentation says stdin is "appended as a `<stdin>` block" and does not mention that an unclosed pipe is a deadlock. The brief is therefore **written to a file and handed over as stdin**, which is codex's documented primary path for instructions and gives EOF for free. It also keeps the argument vector short, which matters on Windows for the reason below.

**`on_path` finding codex is not the same as being able to start it, and this cost a real failure.** The first attempt to drive `generate()` failed with `codex would not start: program not found` *even though* `capability()` had just reported it installed and ready. On Windows the PATH entry is `codex.cmd` and an extensionless shell script; `Command::new("codex")` does not apply `PATHEXT`, and `CreateProcess` cannot execute a `.cmd` or an extensionless script at all. So `spawnable()` resolves the program through `PATHEXT` and, for a `.cmd` or `.bat`, runs it through `COMSPEC` with `/c`. Two tests pin this, and one of them asserts that on Windows the resolved launcher **has an extension**, because an extensionless resolution is precisely the thing that silently fails to spawn.

This is worth stating as a general lesson, not a Windows footnote: **"the binary exists" and "the daemon can run it" are two different facts**, exactly as [14](14-settings-and-providers.md) insists that "installed" and "drivable" are two different facts about a provider. `capability()` answers the first and is right to; only a spawn answers the second.

**codex authenticates per machine and its token expires.** When the refresh token is spent, every request fails with `401 token_expired` and the fix is an interactive `codex login`, which no daemon can do. That is a blocker to report, not an error to raise, which is why a failed generation degrades.

**The model in a user's `~/.codex/config.toml` can be one their account cannot use.** On this machine the configured default `gpt-5.2-codex` returned `400`: "not supported when using Codex with a ChatGPT account", and so did `gpt-5-codex`, `gpt-5.1-codex`, `gpt-5.2` and `codex-mini-latest`. `codex debug models` listed what was actually available: `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`. No model name is hard-coded here, because that list is exactly the sort of thing that rots; `diagnose` turns that specific `400` into a sentence naming `codex debug models` and the `assets.model` setting.

## Optional on the studio's existing terms

Two mechanisms, both already in the repo, and no third one:

- **The capability table.** `assets::blockers(installed, enabled)` returns a list of reasons, each ending in what to do next, exactly like `studio_core::Provider::blockers(needs)`. Nothing infers availability from whether a binary happens to exist.
- **The per-project optional skill.** `skills::ensure_codex_assets` writes `.claude/skills/codex-assets/SKILL.md` into the project the same way `ensure_img2threejs` clones one in, returns `true`/`false`/`Err` and never fails a run.

**Readiness is per kind, because the two halves fail independently and a studio that reported one number would be lying about one of them.** A python project cannot hold a procedural three.js model and can absolutely hold a sprite; a machine without pillow can build models all day and cannot cut a background off. So `Capability` carries three lists — what blocks everything, what blocks drawing, what blocks models — and `blockers_for(kind)` is the only thing that decides whether a request runs. `Capability::ready()` means "some kind is runnable", and the panel disables the button per kind rather than as a whole.

| Blocker | Blocks | Said when |
|---|---|---|
| switched off | everything | `assets.enabled` is `false`. |
| `codex` not on PATH | everything | `on_path("codex")` finds nothing. Names the install command and says the art crew keeps working by hand. |
| no python | drawing | there is no background remover to run. Says a texture needs no cut-out, because that is a real way forward rather than a shrug. |
| codex's imagegen skill missing | drawing | `$CODEX_HOME/skills/.system/imagegen/scripts/remove_chroma_key.py` is not there. |
| the engine has nowhere to put a model | models | the project is `python`, or no engine was detected. Names sprites and textures as what this project *can* have. |

### "A python on PATH" is not "a python that runs", and this cost a real generation

The first real run of the sprite pipeline failed at the last step with:

> the background could not be removed: Python was not found; run without arguments to install from the Microsoft Store...

`on_path("python3")` had found `%LOCALAPPDATA%\Microsoft\WindowsApps\python3.exe`, which on Windows is not an interpreter at all: it is the **App Execution Alias**, a shortcut that prints that sentence and exits 49. The real interpreter was `C:\Python313\python.exe`, second in the candidate list and never reached. This is the same mistake as v0.1's `codex.cmd` spawn failure and the same mistake as the `codex features` probe, for the third time: **presence on PATH was mistaken for capability.**

So python is not resolved by name. `python()` walks `python3`, `python`, `py` and returns **the first one that answers `-c "import PIL"` with exit 0** — the question worth asking, because that is precisely the interpreter the remover needs. A candidate that fails is classified rather than dropped: the Store shortcut is recognised by its own sentence and named as a shortcut, and an interpreter that starts but has no pillow is named with the exact `-m pip install pillow` command for *that* interpreter, not a generic one.

The verdict is cached against the candidate list itself, so `GET /assets` costs one spawn per daemon lifetime rather than one per poll, and installing python changes the candidate list, which invalidates the cache without anything having to watch for it. `windows_store_shortcut_is_recognised_as_the_non_interpreter_it_is` pins the sentence that caused the failure, because it is the kind of string that only a real run produces.

**With the feature off, or with codex absent, every existing path behaves exactly as it does today.** `crew_hint` returns the empty string, so not one byte is added to any charter or brief and no prefix hash moves ([02](02-context-engine.md)). No skill is installed. No file is written into the project. `a_studio_that_switched_this_off_hands_the_crew_no_extra_words_at_all` pins the empty string, because a hint that leaked in when the feature was off would silently cold-start the prompt cache for every art seat.

### The settings keys

| Key | Value | Default | Changes |
|---|---|---|---|
| `assets.enabled` | bool | **`true`** | whether the crew may spend Codex budget on assets |
| `assets.concept` | bool | **`true`** | whether a model is drawn as concept art first and built from that |
| `assets.rig` | bool | **`true`** | whether a character is built as a joint hierarchy with animation clips |
| `assets.model` | free text | **`gpt-5.6-sol`** | the `-m` handed to codex |

Flat keys in `.studio/settings.json`, read through `studio_settings::Settings`, written by the panel through the existing `POST /settings` merge. Nothing new persists anywhere else.

**v0.1 shipped this off and v0.2 ships it on, at the studio owner's instruction.** That is a real trade and it is worth stating what was given up: with it on, a project whose codex is installed and signed in gets the crew hint in every art brief, so the prefix hash moves once, and a crew that asks for an asset spends Codex budget without anyone having ticked a box. The argument for it is that a generator nobody switches on is a generator nobody has, and the crew's alternative is placeholder art. `assets.enabled = false` still turns every part of this off, still empties `crew_hint`, and `a_studio_that_switched_this_off_hands_the_crew_no_extra_words_at_all` still pins that the off state adds not one byte to a brief.

**`assets.concept` doubles the cost of a model on purpose.** With it on, a character costs two Codex requests rather than one: one to draw it, one to build it from the drawing. It is on because a model built from a picture of itself beats a model built from a sentence, and because the picture is a deliverable in its own right. It is off per-request from the panel and per-call through the route.

### The model is always passed explicitly, and never left to codex's config

`-m` is on every invocation. This is not tidiness: on the machine this was built on the user's `~/.codex/config.toml` pins `gpt-5.2-codex`, which **their own account refuses**, so a bare `codex exec` fails for them regardless of this feature. Falling back to "whatever codex is configured to do" would have made the studio's most common failure a stale line in a file it does not own. `model_in` resolves `assets.model`, treats whitespace as unset, and otherwise returns `DEFAULT_MODEL`.

`DEFAULT_MODEL` is `gpt-5.6-sol` — codex's own default and, per its picker, the "latest frontier agentic coding model". The studio does not second-guess the user's tooling about which model it should prefer.

**Model availability is per account and it moves.** No closed list is hard-coded. The panel's model field is free text, and its suggestions come from `GET /models` — the shared per-provider catalogue and probe owned by [14](14-settings-and-providers.md) — read defensively: the panel prefers that route's real shape (`candidates[].verdict`, `detail` for the CLI's own refusal words, `sources[].id` for provenance, `checked_at`, `context_window`, `cost_usd`) and also tolerates several plausible variants. When it understands none of them it offers **no** suggestions rather than inventing any, and when the route is absent entirely the field still works, defaulted, and says no probe has reported yet.

**A catalogued model and a usable model are different things, and this is the distinction the panel has to teach.** For codex the catalogue is free — `codex debug models` is a local call, so names populate at no cost — but a catalogue only says a model *exists*. Only asking it says *this account may spawn it*: this machine's config pins `gpt-5.2-codex`, which the catalogue does not list at all and the account refuses outright. So a model is labelled `verified` only where the probe says so, anything unprobed reads **not checked** rather than working, and the refusal message says in as many words that a merely-catalogued model is only known to exist. Costing a paid probe just to fill a dropdown would be the wrong trade; costing one to answer "will this actually run" is the right one, and that button lives in the settings panel, not here.

**No reasoning effort is passed, deliberately.** `GET /models` reports per-model `efforts` and they genuinely differ — `gpt-5.6-sol` and `terra` reach `ultra`, `luna` stops at `max`, the 5.5 and 5.4 family stop at `xhigh` — and an unsupported model-and-effort pair fails at request time. Since asset generation has no reason to prefer anything over codex's own `default_effort`, the studio sends no effort flag at all and cannot mint an invalid pair. If an effort picker is ever added here it has to be filtered by that model's `efforts` list, not offered as a fixed set.

### The failure this feature will actually hit

A refused model, and it gets its own diagnosis rather than a generic failure:

> codex refused the model gpt-5.2-codex. That is a restriction on this ChatGPT account, not a fault in the studio, and codex said so in these words: "The 'gpt-5.2-codex' model is not supported when using Codex with a ChatGPT account." Pick another in the assets panel: a model the catalogue merely lists is only known to exist, so prefer one the studio has already seen answer.

The model name is in it, the blame is placed where it belongs, **codex's own sentence survives verbatim** so the user can search for it, and it teaches the catalogued-versus-usable distinction at the one moment the user is actually paying attention to it. `a_refused_model_is_named_and_blamed_on_the_account_rather_than_on_the_studio` pins all four properties.

One piece of noise deliberately *not* diagnosed: the user's config registers an MCP server `unityMCP` on `127.0.0.1:8080` which is not running, so every codex invocation logs `HTTP 404 Cannot POST /mcp`. That is their configuration, not this feature's problem, and success is never judged by the absence of stderr — it is judged by the answer file and the mesh count. `an_unrunning_mcp_server_in_the_users_config_is_not_mistaken_for_a_failure` holds that line.

## What a fresh install actually has

This feature quietly grew a dependency list that no installer sets up, and until now nothing told the user. Someone could install the app, have `claude` and `codex`, watch `studiod doctor` print a clean bill of health, and only discover at their first art task that the background remover has no python to run in.

So the doctor reports an **asset pipeline** section beside the coding CLIs, the toolchain and the engines:

| Reported | Needed for | If absent |
|---|---|---|
| `node` | baking a model into the `.glb` an engine imports, and reading a mixamo clip | `winget install -e --id OpenJS.NodeJS.LTS` |
| `codex` | drawing sprites and textures, and writing model source | the installer OpenAI publishes for this OS, then `codex login` |
| `python` with `pillow` | removing the background from a generated sprite | `winget install -e --id Python.Python.3.13`, or `<that python> -m pip install pillow` |
| codex's imagegen skill | the background remover codex ships beside its own image skill | install or update codex and open it once |

`node` was missing from the doctor entirely, and had been since before any of this: `model_export.mjs` has always needed it. That was a real hole and it is closed.

**The report names the interpreter it means.** The first draft of this printed `python3.exe -m pip install pillow` — pointing at Windows' Store shortcut, the same non-interpreter that broke the first real sprite run, so the command it told the user to run would have done nothing. `interpreter_without_pillow()` returns the candidate that actually *starts* and lacks pillow, and the remedy targets that. Presence is not capability, for the fourth time in this document.

**And "actually starts" has to be established by starting it.** The first fix asked the shortcut what it thought of itself: it matched the words *was not found* and *Microsoft Store* in the output. That works on exactly one Windows — an English one. The shortcut announces itself in the machine's own language, so on a Turkish install it says `Python bulunamadı`, the match fails, and the shim is filed as a real interpreter that happens to be missing pillow. The remedy then points `pip install pillow` at a stub that installs nothing and exits 9009, and the real interpreter — second in the `["python3", "python", "py"]` order, one line further down the same PATH — is never named. The localised case is not exotic: it is every Windows outside the English ones.

`runs_python()` decides it by running `-c "import sys"` instead. A genuine interpreter missing pillow still executes that; a stub fails it, in every language. `import PIL` failing only means "pillow is missing" once the thing has proved it runs python at all — so the cheap trivial program is what separates the two, and the message text is never read for a verdict again.

### Optional has to stay optional

An incomplete art pipeline is **not a broken install**, and none of this may read as one:

- It is its own exit code, `4`, distinct from "nothing to code with" (`2`) and "nothing the studio can drive" (`3`). A studio with no CLI has a bigger problem than art and still reports `2`.
- The advice is printed even in the `2` case, because that is the report the installer shows and a user who fixes their CLI should not have to discover the art requirements separately.
- Every line says what it is needed *for*, so a user who does not want generated art can decide not to care.
- The installer's message opens with "installed and ready to code" and says in as many words that the studio works without it.

### Installing the missing pieces, only if asked

`studiod doctor --fix` prints exactly what it will run, one line per tool, and then asks. Answering anything but `y` installs nothing. `--yes` skips the prompt, which is what the installer passes after its own Yes/No dialog — the consent is taken once, in the place the user is already looking.

It never claims to do what it cannot: a tool whose remedy has no runnable command is listed as "nothing to run" with the instruction, and `codex login` is called out separately every time, because a sign-in is the user's to give.

The winget package ids were checked against `winget search` on a real machine rather than recalled, because a confidently wrong package id is worse than no suggestion.

### Nothing is installed through npm that ships its own installer

`claude`, `codex` and `kimi` each publish a standalone build and a one-line installer per operating system, and Copilot publishes one for macOS and Linux and a winget package for Windows. Reaching for `npm i -g` instead would put a node runtime and a `.cmd` shim between the studio and a binary that needs neither — and that shim is not hypothetical, it is the thing `launcher_for` exists to work around, because `CreateProcess` cannot start the extensionless sibling npm drops beside it.

So the remedy is the vendor's own line: `irm …/install.ps1 | iex` on Windows, `curl -fsSL …/install.sh | sh` elsewhere. `gemini` is the exception and still goes through npm, because Google publishes nothing else, and its remedy says so in as many words rather than leaving the odd one out unexplained.

**A piped installer is one argument, not a command line.** The remedy names the shell — `powershell -ExecutionPolicy ByPass -c`, or `sh -c` — and hands it the whole pipeline as a single argv entry, so `doctor --fix` spawns it directly with no shell of its own and no re-parsing. When the same remedy is *printed* — in the report, and in the tab-separated row the Windows installer reads — `command_line()` puts the quotes back around anything holding whitespace or a shell operator, because the installer runs that line through `cmd /C`, where a bare `|` would be cmd's own pipe and `iex` would be started as a program.

## Where a generated asset lands

Not invented here. `plan_for(engine, slug)` reproduces what the engine profiles and the `engine_hint` prose in `wf.rs` already tell workers:

| Engine | factory | then |
|---|---|---|
| `web` | `src/models/<slug>.js` | the browser imports it directly; no export step |
| `godot`, `unity`, `ue5` | `tools/models/<slug>.mjs` | baked to `assets/models/<slug>.glb`, which the engine imports |
| `python`, undetected | — | refused with a reason, and told to ask for a sprite instead |

The factory contract is the one `tools/model_export.mjs` already enforces, because that helper is the verifier: **one default export, taking `THREE` as its only argument, returning a `THREE.Group`, importing nothing.** A factory that imported `three` would be unloadable in node and so unverifiable, which is why `looks_like_a_factory` rejects an import before the file is written rather than after.

For the `.glb` engines the prompt also says textures, canvases and data URIs do not survive the export. That is not a guess; it is what the `godot` profile in [07](07-engine-layer.md) already tells the art crew.

Raster assets take a different path, and **it is the same path on every engine**:

| Kind | Lands at | Cut out |
|---|---|---|
| `sprite` | `assets/sprites/<slug>.png` | yes |
| `texture` | `assets/textures/<slug>.png` | **no** — a texture that got cut out would be a hole where a surface should be |
| the concept art a model was built from | `assets/concept/<slug>.png` | yes |

`image_path_for` does not branch on the engine at all. That is not laziness: a PNG in a project-relative folder is exactly what Godot's `res://`, a browser `fetch`, a Unity import and pygame's `image.load` all consume, and inventing four spellings of the same idea would create four things to keep in step for no gain. Models branch by engine because their *loading* genuinely differs; images do not.

`tileset` and `sprite sheet` are still absent, but the reason has changed: v0.1 refused them because it believed no raster path existed, and that reason is gone. What remains is that both need a project convention for cell size and atlas layout that this studio does not have, and inventing one silently inside an asset generator is how you get art nothing can slice. `AssetKind::from_key("tileset")` still returns `None` and the route still refuses it by name.

## The generation, step by step

```
<launcher> codex exec --skip-git-repo-check --sandbox read-only --color never \
  -C <project> --output-schema <schema.json> -o <answer.json> \
  -m <assets.model> [-i <reference image>]      < <brief.txt>
```

**`--output-schema` is what shipped, and it works.** A statement about a run, not an intention: the real generations below used exactly this, and codex wrote well-formed JSON with exactly the keys `source` and `notes` into `-o`'s file. Stdout capture was the fallback if it had not; it was not needed, so it is not in the code and there is no second path to maintain.

1. **Refuse first.** `Capability::blockers_for(kind)`, then a name that slugifies to something, then a non-empty description, then the no-clobber check. An asset with no description is refused before codex is paid to guess.
2. **Ask.** The answer is `{source, notes}` against `ANSWER_SCHEMA`. stdout and stderr go to `.studio-out/assets/<slug>.codex.log`, a file rather than a pipe, so a chatty run cannot deadlock on a full pipe buffer. The wait is bounded at ten minutes and the child is killed on overrun.
3. **Read the source, not the filesystem.** `parse_answer` refuses anything that is not the schema and quotes the first 200 characters of what arrived instead, because "codex said something else" and "codex said nothing" need different fixes.
4. **Check what can be checked cheaply.** `looks_like_a_factory` rejects a missing `export default`, any `import`, and any `require()`.
5. **Write it, then prove it loads.** `node tools/model_export.mjs <factory> <proof.glb>`. A non-zero exit, an unreadable report, or **a mesh count of zero** is a failure. Zero meshes matters: a factory that returns an empty group exports cleanly and renders nothing.
6. **Degrade, never fail.** On any failure the factory file is **restored to what it was, or deleted if it did not exist**, and the reason names the log. The run continues; nothing raises.

`a_generation_that_cannot_run_leaves_the_project_exactly_as_it_found_it` asserts the refusal path writes neither `src/` nor `.studio-out/`. `generating_while_the_feature_is_off_refuses_without_spending_anything` drives the real route and asserts `200` with `ok: false` — a refusal is an answer, not a server error, because a 500 would make the floor look broken when the studio is merely switched off.

## Drawing one, step by step

```
<launcher> codex exec --skip-git-repo-check --sandbox read-only --color never \
  -C <project> --output-schema <image schema.json> -o <answer.json> \
  -m <assets.model>                              < <brief.txt>
```

The same invocation as above with a different schema, which is why `run_codex` takes an `Ask` rather than a whole `Request`: one spawn path, two briefs.

1. **Ask for one image and nothing else.** The brief names the built-in image generation tool, forbids moving, copying, post-processing and any shell command, and asks for the absolute path in `image_path`. It tells codex the studio does its own copying, because a model that helpfully moves the file breaks the one guarantee below.
2. **Take the path as an untrusted string.** `source_in` requires the extension to be `png` or `webp`, canonicalizes it, and **requires it to resolve inside `$CODEX_HOME/generated_images`**. This is the same shape as `reference_in` and it exists for the same reason: a path that arrives from a model is an input, not a fact. Without it, an answer naming `~/.ssh/id_rsa.png` would be copied into the project and committed by the next daemon-side commit.
3. **Copy it into the working directory, then read its header.** `inspect` parses the PNG signature and IHDR for width, height and whether the colour type carries alpha. No image crate was added for this: the header is 26 bytes and the whole question is "is this a real PNG and does it have an alpha channel".
4. **Cut the background off, unless it is a texture.** `remove_chroma_key.py` with `--auto-key border --soft-matte --despill`, the flag set codex's own skill prescribes. The key colour is `#00ff00`, or `#ff00ff` when the description mentions a green subject — `key_for` reads the words rather than the pixels, which is crude and free and catches the case that actually happens.
5. **Prove the background is gone.** This is the step that matters, and it is why `cutout_check.py` exists rather than trusting the remover's exit code. It reports corner alpha and the opaque share, and `judge` refuses three distinct failures with three distinct sentences: **corners still opaque** means the key never matched and the sprite still has its background; **almost everything opaque** means nothing was removed; **almost nothing opaque** means the remover ate the subject along with the background. A sprite that fails any of these is not written, because shipping a sprite with a green rectangle behind it is worse than reporting that it failed.
6. **Land it.** `land` writes through `inside`, so the destination is still derived from the kind and the slug and can never come from the model.

## The pipeline: codex draws it, then builds it from its own drawing

Asking for a `character` or a `prop` with `assets.concept` on runs both halves in one call:

1. Draw the asset as concept art against the sprite contract plus `concept_shape`, which asks for one unclipped three-quarter view with every part the model needs already visible.
2. Cut its background off and land it at `assets/concept/<slug>.png`.
3. Hand that file back to codex as `-i` on the factory ask, so the brief that builds the model is looking at a picture of the thing it is building.
4. Verify through `model_export.mjs` exactly as before.

Two properties fall out of this and both are deliberate:

**A concept that already exists is reused rather than redrawn.** If `assets/concept/<slug>.png` is on disk, the draw step is skipped entirely and the existing file becomes the reference. A second attempt at a model therefore costs one request, not two, and hand-drawn concept art dropped into that folder is picked up as-is. This is the one place a human can steer the pipeline without touching the route.

**A failed model takes its concept art with it.** If the factory does not verify, the factory file is restored *and* a concept image planted by this same call is deleted. Anything else would leave the project holding art for a model that does not exist, which is exactly the kind of half-state the degrade rule exists to prevent. A concept that was already there is left alone, because this call did not create it.

The failure policy differs between the two halves on purpose: if the **drawing** step fails, the whole call fails and nothing is written. It does not silently fall back to building the model from the description alone, because a text-only model is a different asset from the one that was asked for, and quietly substituting it would hide the failure at exactly the moment the user is deciding whether the feature works. If codex simply *cannot* draw here — no python, no imagegen skill — that is a capability, not a failure, and the concept step is skipped without comment.

Successes are appended to `.studio/assets.json` in the project, keyed by slug so regenerating replaces a row instead of doubling it. That is the same place and shape as `.studio/game.json` ([games](../../crates/studio-server/src/games.rs)).

## Rigging: the part that decides whether a generated character can ever move

A model that cannot be posed is set dressing. Making these characters animatable turned out to be a question about the *file format*, not about the modelling, and the answer narrows the design almost completely.

**The engines load a `.glb`, and glTF animates exactly three things on a node: its translation, its rotation and its scale.** That is `PATH_PROPERTIES` in the vendored `GLTFExporter`, and everything follows from it:

- **Skinned skeletons are not the answer here.** A `SkinnedMesh` needs per-vertex bone weights, and weights are the one part of a rig that procedural code written from a description cannot plausibly get right — a bad weight map reads as melted geometry, and nothing in this studio could check it. There is no verifier for "the elbow deforms nicely".
- **Rigid joint hierarchies are the answer.** Each joint is a bare `Object3D` at the place a limb bends, the meshes hang off it as children, and a quaternion track on the joint swings the whole limb. This is how stylised and low-poly games have always animated, it is exactly what the character contract already asked for ("positioned so an animator can rotate it about a sensible joint"), and it survives the glb round trip natively on every engine that reads glTF.

So the rig is parenting plus keyframes, and both are things a factory can express and a bridge can check.

### The trap that makes this worth verifying

`new THREE.NumberKeyframeTrack('hips.rotation[x]', …)` is the natural thing to write, it is what three.js itself supports in a live scene, and **it cannot be exported**. `GLTFExporter.processAnimation` looks the property up in `PATH_PROPERTIES`, fails to find `rotation`, warns to a console nobody reads and returns `null` — dropping *the entire clip*, not just that track. The glb still writes. The mesh count is unchanged. The file looks fine and the character never moves.

That is the single most likely way this feature fails, so `model_export.mjs` refuses it up front: before exporting, it walks every track, splits the node name from the property, and **exits non-zero if the node is not in the scene or the property is not one glTF can carry**, naming the offending tracks. Nothing is written. The brief carries the same rule in as many words, with `setFromEuler` as the way to get the quaternion you wanted.

### The bridge reports the rig, and the studio judges it

`model_export.mjs` now takes an optional second export, `clips(THREE, root)`, passes the result to the exporter, and then reads the animation names back **out of the glb it just wrote** rather than trusting what it was handed:

```
wrote assets/models/scout.glb (413516 bytes, 85 mesh(es), 2 clip(s))
clips: idle=2.000,walk=1.000
joints: hips,spine,head,upper_arm_l,upper_arm_r,thigh_l,thigh_r
```

The clip count comes from parsing the GLB's JSON chunk, so a clip that was silently dropped cannot be reported as present. `joints:` lists the nodes that clips actually move, which is the only definition of "joint" that means anything: an `Object3D` named `elbow_l` that no track touches is not part of a rig.

Policy stays in Rust. `Proof::missing()` requires every joint in `RIG_JOINTS`, both clips in `RIG_CLIPS`, and a non-zero duration on each clip, and it says which one is absent rather than "the rig is invalid". The old one-line format still parses, so a project whose helpers predate this reports meshes and bytes exactly as before.

### An unrigged character is kept, not thrown away

This is the one place the degrade rule bends, deliberately. If the factory verifies as a model but misses the rig contract, **the model is kept** and the record says `rigged: false` with the specific miss. Discarding it would mean throwing away a concept image and a model the user has already paid for because the animation did not come out — and a static character is a genuinely useful thing that the crew can rig later. The reason names the rig route so the next step is obvious.

### Mixamo: motion comes in, models do not go out

The obvious question about any rig is whether Mixamo can drive it. The answer is asymmetric, and both halves were measured rather than assumed.

**Mixamo cannot rig one of these models, and should not be asked to.** It accepts FBX, OBJ or ZIP and not glTF; its auto-rigger wants one clean humanoid mesh in a T- or A-pose with no existing rig, while a generated character is dozens of separately named meshes that already hang off a joint hierarchy — the exact thing its documentation says to remove first. Even if it succeeded it would discard that hierarchy for its own skinned skeleton, taking the named parts with it. That trade is backwards for this studio.

**Mixamo's animation library can drive them, and now does.** Both rigs are joint hierarchies of the same topology, so the motion transfers; three things had to be solved, and the third was the interesting one.

1. **Names.** Mixamo emits `mixamorig:Hips`; three's `PropertyBinding` strips reserved characters, so what actually arrives is `mixamorigHips`, and some files use `mixamorig1`, `mixamorig5`. `strip()` handles every spelling and `MIXAMO_MAP` covers the 22 bones worth mapping.
2. **Reading the file.** `FBXLoader` is vendored beside `GLTFExporter`, with `fflate` for the deflate-compressed arrays a binary FBX uses and the NURBS curve helpers it imports at module scope. It is driven through `parse(buffer)` directly, so none of three's loading machinery has to exist in node.
3. **Rest pose, which is where a naive retarget silently produces nonsense.**

#### The rest-pose correction, and why the obvious formula is not enough

The textbook retarget is `Rt(t) = Rs(t) · Rs0⁻¹ · Rt0` — take the source joint's world-space delta from its own rest, and apply it to the target's rest. It is correct, it is what "apply the rotation in bone-local coordinates" reduces to algebraically, and **on these rigs it produces garbage**, which the probe caught.

The reason is measurable. A Mixamo rig rests in a T-pose: the upper arm bone points along **+X**. A generated character rests with its arms down: the same bone points along **−Y**. Mixamo bends that elbow with a rotation about world Y — which, applied to a bone that lies *along* Y, is a rotation about the bone's own axis. The arm twists in place instead of swinging. Nothing errors; the clip plays; the character does the wrong dance.

So the delta is conjugated into the target's bone frame before it is applied. For each joint, `boneAim` takes the rest direction from the joint to its first mapped descendant on both rigs, `alignments` builds the swing `A` that carries the target's rest direction onto the source's, and the delta becomes `A⁻¹ · D · A`. Joints with no mapped child — hands, feet, head — inherit their parent's alignment. With that in place a T-pose clip drives an arms-down rig correctly, which is the whole reason no change to the rig contract was needed and every character generated before this still works.

#### What it was measured against

Not a synthetic stand-in: **`Samba Dancing.fbx`, a real Mixamo download**, retargeted onto the real Bell Keeper.

| | |
|---|---|
| source | binary FBX, 3,681,360 bytes, 18.2 seconds |
| joints mapped | **17** |
| result | `idle`, `walk` and `samba` in one glb, 46 meshes untouched |
| agreement | sampling both rigs every two seconds across the whole dance, the **worst joint-angle disagreement is 2.18°** |
| cost | one second of node, and **no Codex request at all** |

The residual two degrees is limb proportion, not error: Bell Keeper's legs are not the dancer's legs, and a knee angle cannot match exactly when the shin lengths differ. The elbow, whose chain proportions happen to line up, agrees to 0.0°.

Two things the importer refuses rather than passing along: an FBX whose clips hold still — every Mixamo export carries an empty `Take 001` — and a retarget that comes back frozen, which is what a rig sharing no moving joint with the animation looks like. Generic clip names (`mixamo.com`, `Take 001`, `Armature|…`) are replaced with the file's own name, so a downloaded `samba.fbx` becomes a clip called `samba` rather than one called `mixamo.com`.

**A web project cannot load a glb**, so when the engine plan has no export step the importer also writes `src/models/<slug>.anims.json` — the clips through `AnimationClip.toJSON` — beside the factory, where the browser can read them.

### Rigging something that already exists

`POST /assets/rig` takes a project and a slug, reads the factory that is already there, and hands codex the whole file with instructions to add the hierarchy and the clips **while keeping every mesh, material and colour exactly as they are**. It is the same spawn path and the same answer schema; what differs is that the brief contains the current source and the verifier compares the result against what was there.

Three guards, because a rewrite of working code is more dangerous than a fresh build:

1. The rewritten source must export a `clips` function, checked before anything is written — a rewrite that quietly dropped the animation would otherwise pass every other check.
2. The rig contract must be met in the exported glb, not merely attempted.
3. On any failure the previous file is **restored byte for byte**, and the reason says the model is back to what it was.

It works on hand-built models too. Nothing in the rig pass requires the factory to have come from codex — only that it is a factory the export bridge can load, which is the same contract the crew's own art already follows.

## What the crew gets

When the feature is ready, `announce` installs the `codex-assets` skill and `crew_hint` adds one paragraph naming it plus **the assets already generated and their paths**, so the second art task loads the scout rather than sculpting a new one. That list now names a drawn sprite by its image path as well as a model by its factory, because a row for a sprite has no factory at all and listing only factories would have hidden every drawn asset from the crew that generated it.

**The hint says what this studio can actually do, not what the feature is called.** With both halves available it offers drawing *and* the pipeline; with python missing it offers drawing's absence honestly by falling back to the models-only sentence. A crew told it can draw when it cannot would burn a task discovering that.

### The crew drives this itself, through one command

Everything above is reachable from the assets panel, which is a person clicking. That is the wrong shape for the crew: a worker cannot click, and a worker that has to assemble `codex exec --output-schema … | remove_chroma_key.py | model_export.mjs` out of prose will get a step wrong and quietly ship a placeholder instead.

So the whole pipeline is one command, `studiod asset`:

```bash
studiod asset sprite    --name "Health Potion" --describe "a round glass flask of glowing red liquid"
studiod asset character --name "Dune Runner"   --describe "a lean desert scavenger in a hooded cloak"
studiod asset rig       --slug dune_runner
studiod asset animate   --slug dune_runner --fbx animations/dance.fbx
studiod asset list
```

It calls the same `generate`, `rig` and `animate` the routes call, so every guarantee in this document holds identically: the destination comes from the engine profile, nothing overwrites what is already there, a sprite whose background survived is refused, a failed rig keeps the model, and the manifest is updated. It prints where each file landed and exits non-zero with the reason when it refuses.

**It deliberately does not call `guard_nested_session`.** That guard exists because a nested `claude` cannot inherit credentials and every worker spawn fails; this command spawns `codex` and `node`, whose sign-ins are their own, and it is *meant* to be run from inside a worker session. The three seats that can reach it — `artist`, `tech_artist` and `audio_designer` — are exactly the ones whose `ToolClass::ArtAudio` allowlist already carries `Bash`.

**The command is named with its absolute path**, resolved through `current_exe()` at the moment the skill is installed and the brief is composed. A worker that has to guess where the binary lives, or hope it is on PATH, is a worker that will decide not to bother.

`crew_hint` therefore carries the invocation itself rather than an announcement that a feature exists, and it says one more thing explicitly: **if the command refuses, build the asset by hand and say in the report that you did and why.** A placeholder nobody mentions is the failure this whole feature exists to prevent, and it is worse than a refusal somebody can act on.

It also states that this comes **before** the img2threejs skill. Both are in a worker's context — [07](07-engine-layer.md) tells the art crew to sculpt from a reference image with img2threejs — and a worker reading two ways to make a model needs to be told which is the default. img2threejs is how a model is built by hand; `studiod asset` is the studio building it for you.

### And in a 3D game, a character that cannot be posed fails the gate

Guidance is not enforcement, so on any engine that has a three.js model path the verify gate audits what the task touched and **fails on a character with no rig**. The failures join `VerifyResult.failures`, which means they go through the existing repair loop: the worker is told what is wrong and gets to fix it, rather than the run stopping.

Four conditions narrow it to the case that was actually asked for, and each one exists to stop a false failure:

1. **The engine must have a model path.** `plan_for` decides this, so a `python` project — or anything 2D — is never audited. There is nowhere to put a rig, so failing for the absence of one would be nonsense.
2. **`assets.rig` must be on.** The same key that makes the generator build rigs makes the gate insist on them; switching it off switches off both, and `assets.enabled` switches off everything.
3. **Only what this task touched.** `git status --porcelain` names the working-tree changes, and since the daemon commits per task, that is exactly this task's work. A static character somebody else left in the tree is not this worker's problem. Without git the audit falls back to every factory, which is the strict reading and is survivable because the fix is one command.
4. **It must read as a character.** Names are extracted from the source's string literals and weighed: at least one of head/torso/chest/pelvis, at least two of arm/leg/thigh/shin/hand/foot, four distinct body words in total. An armchair with an `arm_rest` does not qualify. An explicit `kind` in the manifest always wins over the heuristic, so a statue recorded as a `prop` is never asked to walk.

A model that names a body but exports no `clips` function fails immediately and for free. One that does export clips is put through the export bridge, and `Proof::missing()` decides — so a rig that names its joints but animates none of them is caught too.

The failure carries the fix rather than a complaint:

> src/models/hero.js reads as a character but nothing can pose it: it exports no clips function at all
>
> a character in a 3d game that cannot be posed is set dressing. Run `studiod asset rig --project … --slug hero` to have the studio rig it, or give it the joint hierarchy and clips by hand as the codex-assets skill describes. **If this model is not a character, say so in your report and name it as a prop.**

That last sentence is the escape hatch, and it is deliberate: a heuristic that can be wrong must never be the only way out. A worker who disagrees says so, and a human reads it.

#### It was measured against the models this studio really made

Not only the synthetic fixtures:

| Model | What it is | Gate |
|---|---|---|
| `bell_keeper.js` | rigged by the rig pass | passes |
| `wooden_crate.js` | a prop, no rig, no clips | passes — never asked to walk |
| `dune_runner.js` | a character generated **before** rigging existed | **caught** |

The third row is the point. It was not planted: `dune_runner` came out of the concept-to-model pipeline in an earlier version and genuinely has no `clips` export, and the first real run of the gate found it. The expectation written into the test was wrong; the gate was right.

The skill body is composed from the same `AssetKind::shape()` strings and the same `plan_for` result the daemon uses, so the instructions a worker reads and the prompt the daemon sends cannot drift apart. It now teaches all three steps — draw, key out, build from the cut-out — and it opens by naming the confusion that produced v0.1: `image_generation` is a tool the model calls, so `--help` will never mention it, and `codex features list` is where to look. The art and audio seats already carry `Bash` and `Skill` in their tool allowlist ([04](04-agent-graph.md)), so a worker can drive codex itself; the skill tells it to ask for the answer and write the file itself, for the same sandbox reason the daemon does, and to refuse a sprite whose corners are still opaque rather than ship it.

## Four asset classes

| Kind | Made of | Asked for |
|---|---|---|
| `character` | source | reads as a rig: named head, torso, arm and leg parts about sensible joints, ~1.7 units tall, lowest point at `y = 0`, facing `-Z` |
| `prop` | source | one static object, no limbs, no implied joints, centred on x and z, resting at `y = 0`, under 40 meshes |
| `sprite` | pixels | one subject that survives being lifted off its background: whole and unclipped, generously padded, evenly lit with no separate ground shadow, legible at inventory-slot size |
| `texture` | pixels | a surface sample rather than a portrait: edge to edge, no border, no vignette, straight on, no focal subject, lit so a repeat does not give away the seam |

The difference between two kinds is entirely in what codex is asked for, and it is a real difference in both pairs: a prop asked for with the character prose comes back with legs, and a texture asked for with the sprite prose comes back as one tile floating in the middle of a background.

## Routes

| Route | Does |
|---|---|
| `GET /assets[?project=<id>]` | capability, per-kind blockers and readiness, the four kinds and their prose, where each one's file would land, and the assets already generated |
| `POST /assets/generate` | `{project, kind, name, description, reference?, overwrite?, concept?, rig?}`; `200` with `ok:true` and the record, or `200` with `ok:false` and a reason |
| `POST /assets/rig` | `{project, slug}`; gives an existing model a joint hierarchy and clips, restoring it untouched if the rig does not land |
| `POST /assets/animate` | `{project, slug, animation, name?}`; retargets a downloaded mixamo `.fbx` onto a rigged model. Costs no Codex request, so it is gated on `assets.enabled` alone and not on codex being installed at all |
| `GET /assets/image?project=<id>&path=<relative>` | the bytes of a generated image, so the panel can show what was drawn |

A kind the studio does not make is a `400` that lists the kinds it does.

`GET /assets/image` exists because an asset generator whose output you cannot see is a receipt, not a tool. It is a read of one project-relative path, so it is guarded exactly like a reference: an image extension, `inside()` for the plain-components rule, then canonicalize-and-contain against the resolved project root. It answers `no-store`, because the whole point is that the file changed.

### Two untrusted paths, and they run in opposite directions

`reference` names a file, and that file is **uploaded to OpenAI** as part of the prompt. A bare join against the project root would have let `../../../.ssh/id_rsa` be attached to a request, so `reference_in` refuses in four ways before the filesystem is trusted:

- the extension must be one of `png`, `jpg`, `jpeg`, `webp`, `gif`
- the path may not be absolute, contain `..`, or contain `:`
- it is then **canonicalized and checked to still be inside the canonicalized project root**, which is what catches a symlink pointing out
- and only then must it exist as a file

`a_reference_that_climbs_out_of_the_project_is_never_uploaded_to_codex` drives the escaping cases against a real file planted outside the project. The string checks alone would not be enough — a symlink passes all of them — which is why the containment check is on the resolved path rather than the given one. This mirrors `is_a_plain_file_name` on the music route ([14](14-settings-and-providers.md)), for the same reason: a path from a browser is an input, not a fact.

**`image_path` in a generation answer is the second one, and it is worse**, because it does not come from a browser at all: it comes from a language model, and it is used as the *source* of a file copied into the project and committed by the next daemon-side commit. `source_in` therefore requires the resolved path to sit inside `$CODEX_HOME/generated_images` — not merely to be a readable PNG, and not merely to be outside the project. An answer naming `C:\Users\me\.ssh\id_rsa.png` passes every check that only asks "is this a real file"; it fails the only check that asks "is this a file codex just drew". `a_generated_image_is_only_collected_from_the_folder_codex_writes_to` drives it with a real file planted outside that folder.

## What is verified, and what is not

| Claim | How it is known |
|---|---|
| codex draws raster images through a built-in tool | **read off the installed CLI** (`codex features list`) **and then run for real**, twice by hand and then through `generate()` |
| it draws under `codex exec` with `--output-schema` and no API key | **observed**: a well-formed `{image_path, notes}` answer and a 1254x1254 PNG on disk |
| the chroma-key remover produces real alpha | **observed**: corners at alpha 0, 1,213,137 of 1,572,516 pixels transparent |
| `--sandbox workspace-write` still reports read-only here | **observed** in a real `codex exec` session header |
| the feature's blockers, per kind, and its degrade path | **unit tested** |
| a generated factory lands where the engine expects it | **unit tested** against `plan_for` |
| a sprite, texture and concept land in their own folders | **unit tested** against `image_path_for` |
| the export bridge report is parsed for the mesh count | **unit tested** against a real report line |
| a cut-out that kept its background is refused | **unit tested** against `judge`, all three failure modes |
| a glb drops a whole clip when a track names `.rotation` | **read off the vendored exporter** (`PATH_PROPERTIES`, `processAnimation`) **and then reproduced**: the bridge refuses six such tracks and writes nothing |
| clips and their joints survive into the glb | **observed**: a hand-written rig exported 2 clips over 6 joints, read back out of the glb's own JSON chunk |
| an incomplete rig is named joint by joint | **unit tested** against `Proof::missing`, all four failure modes |
| a clip that never changes a value is refused | **reproduced**: a hand-written frozen clip is rejected and no glb is written |
| a rig pass reparents rather than rebuilds | **done for real**: 46 meshes in, 46 meshes out, 14 joints animated |
| the clips actually move the body | **measured**: playing `walk`, the shin travels 0.212 and the foot 0.398 units against the hips |
| mixamo will not accept one of these models | **read off Adobe's own documentation**: FBX/OBJ/ZIP only, one clean mesh, no existing rig |
| a mixamo clip drives a generated rig | **done for real** with `Samba Dancing.fbx`, 17 joints, worst joint-angle disagreement 2.18° across 18 seconds |
| the rig gate tells a body from a box | **unit tested**, and **run against three real generated models**: it caught the one static character and left the prop and the rigged model alone |
| the gate never fires where a rig cannot go | **unit tested** against a python project and against `assets.rig = false` |
| the naive retarget formula breaks on these rigs | **reproduced**: without the bone-aim correction the elbow twists instead of bending, and the probe fails |
| a binary FBX can be read in node | **done for real**: 3.6 MB deflate-compressed file parsed through the vendored `FBXLoader` |
| the model default, the refusal diagnosis, path containment, no-clobber | **unit tested** |
| the Windows Store python shortcut is not mistaken for python | **unit tested against the sentence a real run produced** |
| a prop is generated end to end | **done for real, through `generate()` itself** |
| a sprite is drawn and cut out end to end | **done for real, through `generate()` itself** |
| the draw-then-build pipeline | **done for real, through `generate()` itself** |
| `run_codex`, the Rust spawn path | **executed against the live CLI**, and it failed the first time |
| the no-clobber refusal | **unit tested, and observed on the real run's second ask** |

The panel has **not** been seen in a browser. There was no connected Chrome on the machine this was built on, so `assets.js` is checked by `node --check`, by the module and panel-host tests in `web.rs`, and by `probes/assets-panel.mjs`, which mounts it under the `floor-dom.mjs` stubbed DOM against a stubbed `/assets` and asserts thirteen properties: that it no longer claims codex cannot draw, that it offers all four kinds, that each one names its own destination, that a blocked kind disables the button and says why, that a generated sprite is *previewed* rather than merely named and that the preview points at `/assets/image`, that switching it off hides the form, and that a failed read is reported instead of swallowed. Nothing is claimed about how it looks.

Writing that probe also found two fidelity gaps in the shared DOM stub that had been quietly weakening every panel probe: `setAttribute` was a no-op, so no probe could ever observe an `src`, a `value` or a `type`; and `innerHTML = ""` left the children in place, so a panel that redraws looked to a probe like a panel that appends. Both are fixed in `floor-dom.mjs`. `probes/settings-repaint.mjs` remains broken for an unrelated reason that predates this work — `stageModules` rewrites `from "/x.js"` but not a dynamic `import("/x.js")`, so it dies resolving `/browse.js`.

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

That first generation was driven by invoking `codex exec` with the flag vector `run_codex` builds, with the write and the verify done by hand through the same two steps the code performs. It proved the codex contract but not the Rust wrapper, so a second one was authorised to close exactly that gap.

One miss worth recording: the lowest point came out at **y = -0.0061**, so it stands 6mm below the ground plane rather than exactly on it.

### The second real generation, through `generate()` itself

A `prop`, driven by the code rather than around it: `a_real_codex_generates_a_prop_that_loads_through_the_export_bridge`, an `#[ignore]`d test that also requires `STUDIO_REAL_CODEX=1` before it will spend anything — two locks, because a test that bills a subscription must not run because someone typed `cargo test -- --ignored`.

| | |
|---|---|
| asset | `prop` named **Wooden Crate** — "a plain wooden shipping crate, planks with visible seams and iron corner brackets" |
| model | `gpt-5.4-mini`, Codex's own "small, fast and cost-efficient" option |
| wall clock | 148.6 seconds |
| cost | **unknown**, same as before: a ChatGPT-plan session reports no figure |
| answer | 2334 bytes of JSON with exactly the keys `source` and `notes`, 2131 bytes of source |
| result | `src/models/wooden_crate.js`, **22 meshes, 4528 bytes** of `.glb` |

Measured afterwards: root group `wooden_crate`, **all 22 children named**, size 1.600 × 1.200 × 1.600, **lowest point exactly `y = 0.0000`** and **centred exactly on x and z at `0.0000, 0.0000`** — the prop contract asks for both and got both, which the character run did not quite manage. No imports, no comments, one default export. The manifest at `.studio/assets.json` recorded the row.

**And this is the run that found the launcher bug.** The very first attempt failed with `codex would not start: program not found` after `capability()` had reported ready. That failure is the whole reason the second generation was worth its cost: nothing in the unit suite could have caught it, because the unit suite never spawns a process. The fix is `spawnable()` above.

The test then asks for the same prop a second time and asserts the refusal, so the no-clobber guard is confirmed against a real generated file and not only against a planted one.

### The third real run: a sprite, drawn and cut out

`a_real_codex_draws_a_sprite_and_the_studio_cuts_its_background_off`, under the same two locks.

| | |
|---|---|
| asset | `sprite` named **Health Potion** — "a small round glass flask of glowing red liquid with a cork stopper and a leather cord" |
| model | `gpt-5.6-sol` |
| drawn | 1254x1254, 1,451,075 bytes, on a flat `#00ff00` background exactly as asked |
| cut to | `assets/sprites/health_potion.png`, 804,637 bytes with an alpha channel |
| measured | **corner alpha 0**, 381,226 of 1,572,516 pixels opaque — a 24% subject share, comfortably inside the 2–98% the judge allows |

The remover sampled the key as `#04f80a` rather than the `#00ff00` that was asked for, which is the whole reason `--auto-key border` is passed alongside `--key-color`: a generated background is *visually* flat and *numerically* is not, and keying on the nominal colour alone would leave a fringe.

### The fourth real run: the pipeline, end to end

`a_real_codex_draws_a_character_and_then_builds_the_model_from_its_own_drawing` — two Codex requests in one call.

| | |
|---|---|
| asset | `character` named **Dune Runner** — "a lean desert scavenger in a hooded sand-coloured cloak with goggles, wrapped boots and a satchel" |
| model | `gpt-5.6-sol` |
| wall clock | 349 seconds for the pair, of which the draw was about 75 |
| concept | 1024x1536 drawn, cut out to `assets/concept/dune_runner.png` at 1,006,242 bytes, **corner alpha 0**, 23% subject share |
| factory | `src/models/dune_runner.js`, 11,904 bytes, built with the cut-out attached as `-i` |
| verified by | `model_export.mjs` → **85 meshes, 413,516 bytes** of `.glb` |

The concept came back as a full-body three-quarter figure — hood, goggles, face wrap, shoulder plate, satchel, wrapped forearms, strapped boots — and the factory that followed names all of it. **85 meshes against the 54 the description-only character run produced**: the model built from a picture of itself is markedly more detailed than the model built from a sentence, which is the entire argument for `assets.concept` defaulting to on.

The manifest row carries both paths, both sizes and `transparent: true`, so the panel can show the art beside the model it produced.

### The fifth real run: rigging a model that was already built

`a_real_codex_rigs_a_model_that_was_already_built_without_restyling_it` builds a static character and then rigs it, so the rig pass is measured against a model it did not write.

| | |
|---|---|
| asset | `character` named **Bell Keeper** — "a stout bronze automaton with a domed head, barrel chest and short jointed arms" |
| built | 46 meshes, 7,586 bytes of source, no clips |
| rigged to | 14 animated joints — hips, spine, chest, head, both upper arms, forearms, thighs, shins and feet |
| clips | `idle` 2.0s over 4 tracks, `walk` 1.2s over 12 |
| meshes after | **46**, exactly what it started with |

The mesh count is the assertion that matters. A rig pass that "helpfully" rebuilds the model is a regeneration wearing a rig's clothes, and the test fails if the count moves by more than a quarter.

**Sampling the clips afterwards is what proved the rig is real, and it caught a hole in the verifier.** Playing `walk` and measuring each joint against the hips: the shin travels 0.212 units and the foot 0.398 — a genuine leg swing — while `thigh_l` itself travels 0.000, which is correct and briefly looks alarming. A joint rotates *in place*; what moves is the chain hanging below it. Measuring the joint's own position tells you nothing, which is a trap worth naming for whoever verifies this next.

The hole: nothing stopped a clip whose keyframes all hold the same value. It would export, count as a clip, satisfy every check, and play as a freeze. The bridge now refuses a clip where no track's values ever change, and says which clips held still.

One contract miss the studio still does not catch: Bell Keeper's lowest point sits at **y = -0.080**, so it stands 8cm into the floor. That is the same gap the earlier character run showed at 6mm, and it is the bounding-box assertion this document has now twice said should be added.

### What these runs cost, and what they did not prove

**Cost is still unknown in dollars.** A ChatGPT-plan session reports no token count and no figure for any of these, so there is none to quote. What can be said is the shape: a drawn asset is one request, a model is one request, and the pipeline is two.

What is still **not** proven: a `texture` generated end to end (only sprites and concepts have been drawn for real), and generation on a `godot`/`unity`/`ue5` project where the factory is baked to a `.glb` the engine imports. Both are wired and unit-tested; neither has been run. `-i` is now proven, because the pipeline uses it on every model it builds.

The character contract asked for `y = 0` and the earlier run got within a centimetre. `verify` does not check this, because `model_export.mjs` reports only bytes and a mesh count and that helper belongs to [07](07-engine-layer.md); a bounding-box assertion is the obvious next thing to add the next time that file changes hands.

`Provider::Codex` in `studio_core` is a different thing to this and deliberately not reused: that is about spawning a *worker* as codex, which the provider table refuses because codex has no flag that replaces a system prompt. This feature invokes codex as a one-shot tool for a single asset, where no frozen charter is involved at all.
