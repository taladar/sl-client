---
id: gridspec-sit-stand
title: Sitting and standing: placement, refusals and alerts on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-sit-stand-actions]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

A child-region sit is refused: SL with the named alert
`SitFailNotSameRegion`, OpenSim the same text unnamed. The fake grid seats at
a fixed offset and silently ignores unknown seats.

## Discover

A `sit-stand` case: sit with and without a sit target, from a neighbour, on
an occupied seat, on the ground; unsit at teleport / logout; both grids.

## Document

`book/src/gridspec/movement.md` § Sitting.

## Fake grid

Small refusals and fallback placement in this task; the full model is
[[server-world-sit-and-attach]].

## Viewer

Named vs unnamed refusal alert, fallback seat position; `e2e` on both flavours.
