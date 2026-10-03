---
id: gridspec-touch-grab
title: Touch and grab on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-sim-script-messages]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Unacknowledged on both grids (the case checks circuit health); the fake grid
does not decode grabs.

## Discover

Run `object-touch-grab` on aditi; grab-drag of an owned non-physical prim;
physical throw; `sl-viewer-ctl world touch` live.

## Document

`book/src/gridspec/building.md` § Touch and grab.

## Fake grid

Large — [[server-world-touch-and-grab]].

## Viewer

Touch / grab via automation on both flavours.
