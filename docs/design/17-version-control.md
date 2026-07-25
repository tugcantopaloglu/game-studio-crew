# 17: Version control

> **Status:** v0.1, 2026-07-25, built and exercised against real repositories.
> **Owns:** everything the studio does with git. The automatic commit after a worker ([01](01-orchestrator-core.md)), the commit tree the floor draws, push and repository creation, and rollback. Emits `commit_recorded` and `git_action` ([05](05-event-protocol.md)); the panel that draws it is part of the floor ([12](12-visual-workspace.md)).

## Principle

**Git is daemon work.** No worker runs git, no worker receives git tools, no commit message is model-generated, and no commit or PR carries a co-author or any other AI attribution. History is written by `studio-core::git` in the daemon's own process, from strings the daemon composed out of the role id and the brief it was given.

This is a token decision before it is a safety one. A worker that can commit has to be told how to commit, which costs prompt bytes on every invocation, and then it spends output tokens narrating what it did. The daemon knows the role, the brief and the working directory already, so a commit costs **zero tokens**. It is also a provenance decision: a worker that can run git can rewrite history, force-push, or write a message that claims a human wrote the code. None of that is reachable, because the tool is not on the worker's `--tools` list ([02](02-context-engine.md)).

## What the daemon does without being asked

| When | What |
|---|---|
| a project is created with git | `git init`, `HEAD` pointed at `main`, a `.gitignore` covering engine and verify artefacts, and one commit so the first worker does not inherit an untracked ignore file |
| a worker completes | stage everything, commit `<role>: <first line of the brief>`, truncated on a word boundary; a clean tree produces no commit |
| a meeting rules | the ADR is written into `docs/decisions/` and committed the same way |

Commits are authored as `Game Studio <studio@localhost>` through `-c user.name`/`-c user.email`, so the studio never depends on, or edits, the machine's git identity.

## What a human can trigger from the floor

Three actions, all of them daemon-side, all reachable only from the git panel:

| Route | Does |
|---|---|
| `GET /git/tree?project&skip&limit` | one page of the commit graph, already laid out in lanes |
| `GET /git/host` | whether the `gh` CLI is present, signed in, and as whom |
| `POST /git/remote` | set `origin` to a URL the human typed |
| `POST /git/create` | create the repository with `gh` and set `origin` to it |
| `POST /git/push` | push the current branch to `origin` |
| `POST /git/rollback` | without `confirm`, the plan; with `confirm`, the reset |

### The tree

The graph is read with **one plumbing call per page**:

```
git log --all --topo-order --skip=<n> --max-count=<limit+1>
        --format=%H<US>%h<US>%P<US>%an<US>%at<US>%D<US>%s
```

`--graph` is deliberately not used: its output is ASCII art meant for a terminal, and parsing it back into structure is a trap. `%P` gives the parents, which is the only thing a layout needs. Fields are separated by `US` (`\x1f`) so a subject containing spaces, tabs or pipes cannot break the parse. One row is asked for beyond the page so the answer can say `more: true` without counting the whole repository — a repo with thousands of commits costs the same as a repo with ten.

Lanes are assigned by the daemon in `git::lay_out`, so the layout is unit-testable and the browser only draws. The algorithm holds a vector of lanes, each expecting one sha:

1. A commit takes the lane already expecting it, or the leftmost free lane.
2. Its first parent inherits that lane; every other parent takes the lane already expecting it, or the leftmost free one. A merge therefore **fans out**, and a branch **rejoins** the lane its fork point already occupies.
3. Every lane that survives the row produces a link `(from, to)`; every parent produces a link from the commit's own lane. The links are what the panel strokes between one row and the next.

Each row carries its lane, its links, the short and full sha, subject, author, unix timestamp, and the refs pointing at it (`%D`, so `HEAD -> main`, `origin/main` and `tag: v0.1` all arrive together). Rows are drawn as inline SVG, one small element per row, which keeps paging cheap and makes the tree scroll like a list rather than repaint like a canvas.

**Known limit:** each page is laid out on its own, so a commit whose child is on the previous page starts a fresh lane rather than continuing one. Within a page the graph is exact.

### Push, and creating the repository

`push` resolves `origin` (or the only remote, if it is named something else), refuses a detached HEAD, and runs `git push --set-upstream` with `GIT_TERMINAL_PROMPT=0` so a credential prompt can never wedge the daemon on a machine with no terminal attached. **Whatever git said is what the floor shows** — stdout and stderr, trimmed, success or failure. A rejection is reported as a rejection, with one added line naming the way out for the three cases worth naming: a non-fast-forward wants a fetch, an auth failure wants `gh auth login` or an SSH remote, an unreachable host wants the network checked. Nothing else is interpreted, and success is never claimed for a push that was not observed to succeed.

With no remote at all, push does not repeat git's `fatal:`; it says the project has no remote yet and names both ways to get one. If `gh` is on PATH and signed in, the panel offers to create the repository (`gh repo create <name> --private|--public --source . --remote origin`) and then pushes as a separate, separately reported step. Otherwise it takes a URL.

**The studio never stores a credential.** A remote URL carrying one — `https://ghp_…@host/…` or `https://user:secret@host/…` — is refused before it is written to `.git/config`, and the refusal does not echo the secret back. Authentication is the git credential helper's job or `gh`'s, never the studio's.

### Rollback

Rollback is `reset --hard` followed by `clean -fd`, which is the most destructive thing the studio can do, so it happens in two steps.

A `POST /git/rollback` without `confirm` returns a **plan** and touches nothing: the target commit and its subject, every commit between it and `HEAD` that is about to be thrown away, and every path with uncommitted changes — including untracked files, which die in the clean and are the ones nobody remembers. The panel shows the commits and the dirty paths, the dirty ones in red, and only then offers the button that resets.

The target is validated twice: it must look like a sha (hex, 6-40 characters, so a branch name or a wildcard cannot be smuggled in) and `cat-file -t` must call it a commit, so a well-formed sha that is not in this repository, or a tree sha, is refused before anything moves. `.claude` and `.studio-out` are excluded from the clean, so the studio's own working files survive a rollback of the game.

## Events

Push, remote creation and rollback each emit `git_action` ([05](05-event-protocol.md)):

```json
{"project": "proj_dash", "action": "push", "ok": false, "detail": "push to origin was rejected: ..."}
```

Failures are announced as loudly as successes — a floor that only hears about the pushes that worked is lying by omission. They are persisted under the run id `git` rather than a worker run, because they belong to the project and not to any one run, and broadcast live, so the panel refreshes its tree the moment anything moves. A preview is not an action and emits nothing.

## Why no worker is ever allowed near it

- **Cost.** Git instructions in a charter are paid for on every invocation of that role, forever. The daemon already knows everything a commit needs.
- **Attribution.** A model that writes commit messages eventually writes one that mentions itself. The daemon composes the subject from the role id and the brief, and a test asserts the log never contains "laude", "Co-Authored" or "Generated with".
- **Irreversibility.** Push and reset are the two operations in this system that a human cannot undo from the floor. Both are behind a human's click, and reset is behind a human's click on a list of exactly what it destroys.
- **Provenance.** History is the record of what the studio did. It is written by the part of the studio that knows what happened, not by the part that was asked to guess.
