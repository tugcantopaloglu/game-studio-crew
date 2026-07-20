# ADR 0005: A voxel 3D floor, not a 2D orthogonal one

> **Status:** Accepted, 2026-07-20. **Supersedes the projection choice in [ADR 0003](0003-top-down-not-isometric.md)**; that ADR's data-model argument is unchanged and is what made this cheap.
> Context for [12](../12-visual-workspace.md).

## Context

[ADR 0003](0003-top-down-not-isometric.md) chose a top-down orthogonal 2D grid over isometric, on three grounds: cheap hit testing, flat legible text, and CC0 asset availability. It then wrote down an escape hatch: the spatial model is projection-independent, the store is a pure reduction over the event log, and the event→visual mapping references *what* happened rather than *how it is drawn*, so a projection change is "a renderer change only".

M4 shipped that 2D floor. Seen running, it reads as coloured circles in grey boxes: correct, legible, and lifeless. It conveys state without conveying that a *studio* is at work, which is the thing [00](../00-overview.md) says the floor exists for. The user asked for a 3D scene with a real pixel-art model per character.

## Decision

Render the floor as a **3D scene of voxel characters** (Three.js, WebGL), with an isometric-style camera by default and a free orbit toggle.

- **Characters are real voxel models.** Each of the 13 roles is built from ~280-310 unit cubes in one `InstancedMesh`, so the cube grid is visible and every character is genuinely 3D geometry rather than a billboarded sprite. Per-role palette plus headgear (crown, hard hat, beret, headset, cap, headphones) and a held prop (wrench, brush, clipboard, magnifier, quill, ruler, tablet).
- **Models are generated from box ranges expanded into unit voxels**, not hand-authored layer by layer. The output is true voxel data; the input stays compact enough to edit.
- **The status ring survives the move.** It is now a glowing ring on the floor under each character. [12](../12-visual-workspace.md)'s rule holds unchanged: shape encodes tier, palette encodes department, and *only* the ring encodes runtime state.
- **Three.js is vendored**, not loaded from a CDN, so the floor works offline and the daemon has no runtime network dependency.

## Why the escape hatch held

Nothing behind the renderer changed. The deterministic shelf packing in `studio-agents` produces the same grid coordinates; the 3D scene reads `x`, `y`, `w`, `h` and interprets them as world units instead of screen pixels. The event protocol, the coalescer, the snapshot, the store, and every test over them were untouched. **This is the entire value of the discipline ADR 0003 imposed**: an aesthetic reversal that would otherwise have been a redesign cost one file.

## What ADR 0003 got right and wrong

- **Right, and still binding:** keeping projection out of the data model. That is why this ADR is short.
- **Wrong on hit testing:** the argument was that orthogonal hit testing is a divide while iso needs inverse projection. In 3D it is a raycast, which is one call against the character meshes. The concern was real for hand-rolled 2D canvas and evaporates with a scene graph.
- **Wrong on text:** labels are canvas textures on floor-aligned planes, still flat and legible. Nearest-neighbour filtering keeps them crisp at the pixel-art scale.
- **Right on CC0 assets, and routed around:** good isometric office art is indeed scarce. Generating voxel models in code sidesteps sourcing art entirely, and it keeps the models deterministic and diffable, which downloaded sprite sheets would not be.

## Consequences

- The floor now needs WebGL. A machine without it gets nothing rather than a degraded 2D view. Acceptable for a local developer tool; a fallback is not planned.
- The vendored Three.js is 1.27 MB, served once and cached. It dwarfs everything else the daemon serves, which is the honest cost of the decision.
- Doc 12's performance ladder needs restating in 3D terms (instance counts and shadow map size rather than RenderTexture LOD bands). The ordering principle is unchanged: **status rings and blocked indicators are the last thing to degrade.**
