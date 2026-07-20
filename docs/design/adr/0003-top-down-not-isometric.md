# ADR 0003: Top-down orthogonal, not isometric

> **Status:** Accepted (design phase), 2026-07-20
> Context for [12](../12-visual-workspace.md).

## Context

The studio floor ([12](../12-visual-workspace.md)) renders the crew as a spatial office. The obvious "game studio" aesthetic is **isometric** (think classic tycoon/management sims). The alternative is a **top-down orthogonal grid**. The projection choice ripples into hit-testing, text, and asset sourcing, so it's worth a record.

## Decision

**Top-down orthogonal grid at 32px tiles.** Not isometric.

## Why

1. **Hit testing.** Orthogonal screen→grid is a divide (`floor(x/32), floor(y/32)`). Isometric needs an inverse affine projection plus depth-order disambiguation to answer "which desk did I click." The floor is interactive (hover bubbles, follow mode, click-to-focus, [12](../12-visual-workspace.md)), so cheap, exact hit testing matters.
2. **Text placement.** Desk labels, the live hover bubble, meeting speech, and the spend readout ([12](../12-visual-workspace.md)) all need flat, legible text. Orthogonal lays text on-grid unskewed. Isometric either skews text with the projection (ugly, hard to read) or billboards it upright (which fights the projection visually).
3. **CC0 asset availability.** Top-down 32px office/furniture tilesets are abundant under CC0 (Kenney and peers, [12](../12-visual-workspace.md)). Good isometric office sets at a consistent scale are scarce, which would mean commissioning or compromising art. A real cost for a side-panel view.

Isometric's one advantage, a richer "3D-ish" look, doesn't pay for those three costs in a functional monitoring view whose job is legibility under load.

## The escape hatch

The decision is **reversible without touching the data model.** The spatial model, grid coordinates, rooms, desks, the deterministic shelf packing ([12](../12-visual-workspace.md)), is **projection-independent**. The store is a pure reduction over the event log ([05](../05-event-protocol.md)), and the event → visual mapping ([12](../12-visual-workspace.md)) references *what* happened, not *how it's drawn*. So a later move to isometric is a **renderer change only**: reproject the same grid coordinates, re-source props. Packing, hit-testing logic (re-derived from the new projection), event mapping, and every other doc are untouched.

## Consequences

We commit to top-down now for the reasons above, and we keep the door open by never letting projection leak into the data model. If the aesthetic case for iso ever wins, it's a bounded frontend task, not a redesign.
