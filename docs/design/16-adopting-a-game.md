# 16: Adopting a Game

> **Status:** v0.2, 2026-07-25. Built and wired: the games panel lists every project the studio knows, `POST /games/adopt` registers a game the crew did not build, and `POST /games/summarize` produces a one-worker read of it that is cached in the game's own folder. **The summary is the only part of this that spends tokens**, and it spends them at most once per change to the game. v0.2 split the panel's two questions across two endpoints after a latency sweep found the first one costing 109ms; see *Two endpoints, because two costs*.
> **Consumes** engine detection ([11](11-index-and-bootstrap.md)), the project row in the state store ([03](03-state-store.md)), the role registry ([04](04-agent-graph.md)) and the `game_summarized` event ([05](05-event-protocol.md)).

## The library

A project is a directory the studio works in ([README](../../README.md)). The games panel is the view of all of them at once: name, engine, absolute path, whether it is a git repo and how deep its history goes, when it was last worked on, and where it came from. Selecting a card selects the project for the whole floor, so the dispatch box, the run panel and every command that follows point at the game you are looking at.

Two facts on a card are derived rather than stored, because the projects table holds neither:

- **When it was last worked on** is the timestamp of the last commit when the game is a repo, and the folder's own mtime when it is not. The store does track `last_used`, but `ProjectRow` does not carry it out, and a commit date is the more honest answer anyway: it says when the *game* last changed, not when someone last clicked on it.
- **Whether the studio built it** comes from the marker adoption writes (below). For projects that predate the marker, the fallback reads the repo's root commit: the daemon writes every commit itself with a subject of `<role>: ...` or `crew: ...` ([README](../../README.md)), so a root commit in that shape means the crew built the game from nothing. Anything else means it arrived with a history the studio did not write. A folder with no marker and no git history reports **origin unknown** rather than guessing.

## Two endpoints, because two costs

Both derived facts above cost a `git` subprocess, and the first version of this panel paid for them inside the list. Measured on a two-repo library: **`GET /games` cost 108.93ms p50**, against 1.60ms for `/projects` and 1.68ms for `/roles` on the same harness. Three subprocess spawns per project — `rev-list --count`, `log -1`, and the root-commit read — is most of a tenth of a second on Windows, and the panel pays it every time it opens.

The library and the detail are now separate questions with separate prices:

- **`GET /games`** answers what the panel needs to draw a card: name, engine, path, whether `.git` exists, whether the folder still does, the origin marker, and the cached summary text. Every one of those is a stat or a single small file read. **No subprocess, and no tree walk.** It now costs **1.97ms p50**, level with the cheapest endpoints on the floor.
- **`GET /games/detail`**, optionally narrowed with `?project=<id>`, answers what costs money to know: commit count, last-worked timestamp, history-derived origin, and whether the cached summary is still current. The panel fires it immediately after drawing, so the cards appear at once and the history fills in a beat later. Until it lands a card says "a git repo" rather than a commit count, and a summary is never labelled stale on a guess.

**The detail is cached on the git HEAD pointer, read as a file rather than asked of git.** `.git/HEAD` names a ref, that ref file holds a sha, and `packed-refs` covers the case where it has been packed away — all plain reads costing microseconds. Commit count, last-commit date and root-commit origin are all functions of the HEAD commit, so while that pointer is unchanged the cached answer is exactly right, and when it moves the cache entry for that one project is recomputed. Measured: **121.9ms for the first detail call, 2.4ms for every one after; a commit into one of the two games cost 65.9ms on the next call and 2.4ms after that** — the untouched game stayed cached. Adoption clears the entry too, since the marker it writes overrides the history-derived origin.

**Summary freshness is deliberately not cached**, because the working tree can change without a commit and the HEAD pointer would not notice. It is recomputed on every detail call, which costs 0.35ms on top for a game that has a summary — cheap enough that a second invalidation mechanism would be machinery bought for nothing.

No background thread does any of this. The cost is paid on the request that asks for it, once per change.

## What adoption means

Adoption is registration and nothing else. It records that a directory which already contains a game is now a project the studio may work in. It writes exactly one file that was not already there:

```
<game>/.studio/game.json
```

which holds the origin marker and, later, the cached summary. That path is inside a dotted directory, so the index skips it ([11](11-index-and-bootstrap.md)) and it never enters a survey or a fingerprint.

**Adoption does not scaffold.** This is a separate route from `POST /projects` for one concrete reason. `studio_engine::scaffold` is safe when the engine is *detected*, because detection needs the same marker file the scaffold refuses to overwrite: a directory that detects as Godot already has `project.godot`, so `scaffold_godot` returns immediately. It stops being safe the moment a human names the engine by hand. A browser game whose entry point is `game.html` does not detect as `web`, and creating it as `web` writes `index.html`, `src/main.js`, `package.json`, a `vendor/` tree and a `.gitignore` straight over the top of the game that was already there. `POST /games/adopt` never calls `scaffold` and never calls `install_helpers`, so nothing in the folder is created, replaced or appended.

The tests pin this from the outside: an occupied directory adopted with a hand-picked engine comes out of the route byte-for-byte identical, `.gitignore` included, and no `index.html` appears.

**Adoption does not initialise git unless asked.** If the folder is already a repo, it is recorded as one. If it is not, adoption leaves it alone by default, and the card says plainly that nothing can be reverted. A checkbox offers `git init`, because the daemon's commit-per-worker and revert story ([README](../../README.md)) needs a repo to work at all — but it is a choice made in the open, not a side effect of pointing at a folder.

## When detection finds nothing

Engine detection is a marker set per profile ([11](11-index-and-bootstrap.md)), and a real game frequently trips none of them. That is a dead end only if the studio refuses to say what it looked for. `POST /games/adopt` with `engine: "auto"` and no match answers with the whole marker list — `project.godot` for godot, `ProjectSettings/ProjectVersion.txt` for unity, `*.uproject` for ue5, `index.html` for web, `main.py` for python — and tells the human to pick one. `GET /games/engines` serves the same list to the panel, so the engine dropdown carries each engine's markers as hover text.

Choosing an engine by hand is a claim about which toolchain can build and run the game, and the studio believes it. That is the correct trade: the alternative is refusing to work on a game because its entry point has a different filename.

## What the summary is

A glimpse. Three or four sentences on what the game is and how it plays, plus at most six named mechanics with one clause each, named the way the code names them. It renders on the game's card, and it is what lets somebody looking at a library of eight games remember which one is which.

It is **not** a design document, not a feature inventory, and not a substitute for the index. Nothing downstream reads it: no brief is built from it, no worker is given it, no plan depends on it. It is for the human at the floor. If it is wrong, the cost of it being wrong is that a card reads oddly.

The input is `survey`, the same free structural read the build planner uses ([README](../../README.md)): the file list with sizes, the head of `README.md` / `design/spec.md` / `qa/report.md`, the detected engine and the last few commit subjects. The worker is told to use only that evidence, not to describe the folder layout, and not to invent features the files do not show.

## What it costs

**One worker call, held to a schema.** The role is `game_designer` ([04](04-agent-graph.md)), run advisory: no tools, no allowed tools, so the invocation carries the empty tool set that costs 184 tokens of schema instead of 22572 ([02](02-context-engine.md)). The output is constrained by `--json-schema` to `{summary, mechanics[{name, note}]}` with `maxItems: 6`, so the model cannot answer with an essay and the daemon does not have to parse prose. The worker does not commit, because reading a game changes nothing in it.

**And then it costs nothing again until the game changes.** The result is written into `.studio/game.json` with a fingerprint of the game's contents. Asking a second time compares the stored fingerprint against a fresh one:

- **Fresh** — `POST /games/summarize` returns the cached summary with `200` and never dispatches a command to the daemon. No worker is spawned. The panel says so.
- **Stale** — the card keeps showing the old summary and labels it stale, in the warning colour, with how long ago it was read. It is not silently deleted and it is not silently re-billed; re-reading is a button the human presses.
- **Missing** — the route dispatches `StudioCommand::Summarize` and the daemon spends the one worker.

The fingerprint is an FNV-1a fold over every non-skipped file's path, size and — for files at or under 128 KiB — its contents. It skips the same VCS, editor and build directories the survey skips, plus every dotted directory, which is what keeps writing the cache from invalidating the cache. Hashing contents rather than just sizes is what makes an edit that preserves file length still read as a change; a test pins exactly that, because a tuning pass that turns `100` into `900` is the most likely edit a game gets and the cheaper size-only gate would have missed it.

The cost of the gate itself is a walk plus a read of small files, and it is paid **only for games that already have a cached summary** — a game nobody has read is never fingerprinted, so listing a library of fresh projects touches no file contents at all.

## Events

Both paths emit `game_summarized` ([05](05-event-protocol.md)) with `project`, `summary` and `mechanics`, plus `cached` so the floor can tell a free answer from a billed one. `project` carries the project id rather than its path, because the panel keys its cards on the id. The event is never coalesced ([05](05-event-protocol.md)), so a summary that lands during a busy run still reaches the card.
