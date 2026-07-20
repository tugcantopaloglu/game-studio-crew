# ADR 0002: Thirteen roles, not forty-nine

> **Status:** Accepted (design phase), 2026-07-20
> Context for [04](../04-agent-graph.md). The role registry itself lives in [04](../04-agent-graph.md); this ADR is the argument.

## Context

The original crew had **49** agents. Three design passes proposed 49, 20, and 13. We need one number, and the number has both a correctness dimension (are roles distinct?) and an economic dimension (what does each role cost the cache?).

## Decision

**13 roles.** Engine (Unity / UE5 / Godot) is a **prompt layer** ([07](../07-engine-layer.md)), not a role axis. Rare specialisms are **append-position overlay fragments** ([04](../04-agent-graph.md), [07](../07-engine-layer.md)), not standing roles.

## The consolidation argument

The dominant cause of 49 was **triplication**: "gameplay engineer," "tech artist," and most other roles existed once per engine. But a gameplay engineer's *mandate*, the boundary of their responsibility, their tool contract, their escalation parent, is identical across engines; only the *idioms and tooling* differ, and those are exactly what the engine profile's L1 prose and command lines encode ([07](../07-engine-layer.md)). So the engine dimension collapses without losing anything a role charter needs to say.

A role earns its place only if its **charter, tool allowlist, or escalation position** differs materially from every other role ([04](../04-agent-graph.md)). Applying that test to the de-triplicated set lands at 13. The 20-role proposal kept a few specialisms as standing roles (netcode engineer, shader artist); we demote those to overlays because they don't warrant a *standing* charter. They're occasional context, applied by trigger to a base role.

## The cache-fragmentation economics

This is the decisive argument beyond mere tidiness. Each role is a **distinct frozen system-prompt prefix** ([02](../02-context-engine.md)), and prompt caching is keyed on exact prefix bytes with a **5-minute TTL** ([02](../02-context-engine.md)). Within any 5-minute window, spawns that share a prefix hit cache; spawns of a *different* prefix pay the ~1.25× cache-write premium.

More roles → more distinct prefixes → fewer same-prefix spawns inside any 5-minute window → **colder caches and more `cache_creation` billing.** Concretely: with 13 roles the studio churns through 13 possible prefixes, so a busy sprint keeps most of them warm; with 49 (and per-engine triplication) the same workload is spread across 3× as many prefixes, so far fewer land inside a live window. Fewer roles is not just cleaner. It is *cheaper per token*, directly, via cache warmth. This economic pressure is why overlays (which never touch the frozen prefix) are the right home for specialisms: an overlay adds capability without minting a new prefix.

## What we give up

- A netcode task on a `gameplay_engineer` gets netcode guidance via an overlay in L3, not a bespoke `netcode_engineer` charter. If a specialism ever needs a genuinely different *mandate or tool contract* (not just extra context), it graduates to a 14th role. The test in [04](../04-agent-graph.md) is the gate, and adding a row + charter is cheap.
- Slightly more logic in the trigger/overlay system ([07](../07-engine-layer.md)) than a flat role list would need. A fair trade for the cache economics.

## Consequences

13 is the number the floor packs deterministically from ([12](../12-visual-workspace.md)), the number the ledger groups `cache_hit_ratio` by ([03](../03-state-store.md)), and the number every other doc references. The registry in [04](../04-agent-graph.md) is the single place it's enumerated.
