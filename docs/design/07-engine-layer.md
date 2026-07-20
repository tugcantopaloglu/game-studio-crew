# 07: Engine Layer

> **Status:** v0.1, 2026-07-20, design phase, no runtime code.
> **This document is the single source of truth for the engine profile TOML schema and the three filled profiles.** [08](08-verification.md) names a parser for each report format these commands emit; [09](09-workflows.md) and [11](11-index-and-bootstrap.md) reference profiles by id. This is why there are **13 roles, not 49** ([04](04-agent-graph.md)): an engine is this layer, not a role axis.

## What an engine profile is

A TOML file, one per supported engine, split into two halves:

- **Machine half** (`[commands]`, `[reports]`, `[detect]`): exact command lines the **daemon** runs. Workers never run these; they request verification and the daemon invokes [`EngineDriver::verify()`](08-verification.md).
- **Prompt half** (`[prose]`): prose fragments injected into charters. `[prose].profile` becomes **L1** ([02](02-context-engine.md)). Engine idioms and conventions, part of the frozen prefix. `[prose].capabilities` are **capability fragments** placed only in the **volatile L3 suffix**, never the prefix (see [§capability fragments](#capability-fragments)).

The split is the point: command lines change (a CI flag, a tool path) without touching a single charter byte, and prose changes without touching the daemon's exec path.

## Schema

```toml
schema_version = 1
id = "unity"                     # stable; referenced by workflows, index, ledger
display_name = "Unity"
min_editor_version = "2022.3"

[detect]                          # see 11-index-and-bootstrap for precedence
markers = ["ProjectSettings/ProjectVersion.txt", "Assets/", "Packages/manifest.json"]
precedence = 10                   # higher wins when multiple engines detected

[tooling]                         # how the daemon resolves the binary; see 11
resolver = "unity_hub"            # unity_hub | path | env
binary_env = "UNITY_EDITOR"       # fallback env var holding the editor path

[commands]                        # {project} {tests} {out} {platform} are daemon-substituted
compile   = "..."                 # build/typecheck only, no tests
test_fast = "..."                 # edit-mode / unit tests, fast
test_full = "..."                 # play-mode / integration, slow
import    = "..."                 # asset import / project reimport
export    = "..."                 # cook / build player / package

[reports]                         # each command's machine-readable output; parser named in 08
test_fast = { format = "nunit3", path = "{out}/results-editmode.xml" }
test_full = { format = "nunit3", path = "{out}/results-playmode.xml" }
export    = { format = "unity_buildreport", path = "{out}/buildreport.json" }

[prose]
profile = """L1 engine idioms, injected into the frozen charter prefix.
No engine-specific task detail, no command lines, no versions that churn."""

[prose.capabilities]              # substring-triggered L3 overlays; NEVER in the prefix
netcode = "…guidance appended only when the task text matches a netcode trigger…"
shaders = "…"
```

Charter composition and hashing follow [02](02-context-engine.md) exactly: `[prose].profile` is L1, it is byte-normalized, carries no timestamps or `{{` markers, and is folded into the blake3 prefix hash. **A profile edit to `[prose].profile` changes the prefix hash and correctly cold-starts the cache for that engine's roles; a `[commands]` or `[prose.capabilities]` edit does not.** That is the invariant [08](08-verification.md) and [02](02-context-engine.md) are checked against together (verification #1).

### Capability fragments

Rare specialisms are overlays, not roles ([04](04-agent-graph.md)) and not prefix content. A capability fragment lands in L3 **only when the task brief text matches its trigger** (substring match on the task title/description). It is in the volatile suffix precisely so that adding netcode guidance to one task does not fragment the frozen prefix cache for every other worker of that role. Triggering is deterministic and logged.

## The three profiles (filled, real command lines)

Command lines below are the real invocations; `{…}` placeholders are daemon-substituted. Every profile fills all five commands and names the report format for the ones that produce machine-readable output, which is what [08](08-verification.md) parses (verification #4).

### Unity (`id = "unity"`)

Unity serializes the editor lock, so `test_full` is effectively one concurrent op per project ([13](13-risks.md)). A `Studio.CI` static-method helper (installed by [11](11-index-and-bootstrap.md)) wraps import/export so the daemon drives them via `-executeMethod`.

```toml
[commands]
compile   = "{editor} -batchmode -quit -projectPath {project} -executeMethod Studio.CI.Compile -logFile {out}/compile.log"
test_fast = "{editor} -batchmode -runTests -projectPath {project} -testPlatform EditMode -testResults {out}/results-editmode.xml -logFile {out}/test-edit.log"
test_full = "{editor} -batchmode -runTests -projectPath {project} -testPlatform PlayMode -testResults {out}/results-playmode.xml -logFile {out}/test-play.log"
import    = "{editor} -batchmode -quit -projectPath {project} -executeMethod Studio.CI.Reimport -logFile {out}/import.log"
export    = "{editor} -batchmode -quit -projectPath {project} -executeMethod Studio.CI.BuildPlayer -buildTarget {platform} -logFile {out}/export.log"

[reports]
test_fast = { format = "nunit3", path = "{out}/results-editmode.xml" }
test_full = { format = "nunit3", path = "{out}/results-playmode.xml" }
export    = { format = "unity_buildreport", path = "{out}/buildreport.json" }
```

### Unreal Engine 5 (`id = "ue5"`)

Build via UBT (`Build.bat`), cook/package via `RunUAT.bat BuildCookRun`, tests via the editor automation runner with a JSON report.

```toml
[commands]
compile   = "{ue_root}/Engine/Build/BatchFiles/Build.bat {target}Editor {platform} Development -project={uproject} -waitmutex"
test_fast = "{ue_editor} {uproject} -ExecCmds=\"Automation RunTests {suite}.Unit; Quit\" -unattended -nop4 -nosplash -ReportOutputPath={out}/automation"
test_full = "{ue_editor} {uproject} -ExecCmds=\"Automation RunTests {suite}; Quit\" -unattended -nop4 -nosplash -ReportOutputPath={out}/automation"
import    = "{ue_editor} {uproject} -run=ImportAssets -source={source} -unattended -nop4 -nosplash"
export    = "{ue_root}/Engine/Build/BatchFiles/RunUAT.bat BuildCookRun -project={uproject} -platform={platform} -clientconfig=Development -cook -stage -pak -archive -archivedirectory={out}/build"

[reports]
test_fast = { format = "ue_automation_json", path = "{out}/automation/index.json" }
test_full = { format = "ue_automation_json", path = "{out}/automation/index.json" }
```

UE `.umap`/`.uasset` are binary. The index and trust model treat them specially ([10](10-standards-and-trust.md), [11](11-index-and-bootstrap.md), [13](13-risks.md)). The automation report schema drifts across 5.x, which is why [08](08-verification.md)'s parser is defensive.

### Godot 4 (`id = "godot"`)

Fully headless, no editor lock. The cheapest engine to run and the M3 target ([00](00-overview.md)). Script-check for compile, GUT for tests (JUnit XML output).

```toml
[commands]
compile   = "{godot} --headless --path {project} --check-only --quit"
test_fast = "{godot} --headless --path {project} -s addons/gut/gut_cmdln.gd -gdir=res://test/unit -gexit -gjunit_xml_file={out}/gut-unit.xml"
test_full = "{godot} --headless --path {project} -s addons/gut/gut_cmdln.gd -gdir=res://test/integration -gexit -gjunit_xml_file={out}/gut-integration.xml"
import    = "{godot} --headless --path {project} --import --quit"
export    = "{godot} --headless --path {project} --export-release {preset} {out}/build/game"

[reports]
test_fast = { format = "junit", path = "{out}/gut-unit.xml" }
test_full = { format = "junit", path = "{out}/gut-integration.xml" }
```

## Report-format coverage summary

Every report format above has a named parser in [08](08-verification.md):

| Engine | test_fast / test_full format | export format |
|---|---|---|
| unity | `nunit3` | `unity_buildreport` |
| ue5 | `ue_automation_json` (defensive) | *(cook: exit-code + log scan, no structured report)* |
| godot | `junit` | *(export: exit-code + log scan)* |

`compile` and `import` verdicts come from exit code plus a structured log scan in every engine (no separate report file); the driver contract in [08](08-verification.md) defines how exit-code-only verdicts are turned into `Failure`s.
