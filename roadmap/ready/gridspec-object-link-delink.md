---
id: gridspec-object-link-delink
title: Link order, limits and refusals on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

First id is the root; OpenSim links same-owner only. SL link order ("the prim
linked last is 2") and limits are unmeasured.

## Discover

Run `object-link-delink` on aditi; probes for link order, too-far / too-many
prims and their refusal, delink of the root.

## Document

`book/src/gridspec/building.md` § Linking.

## Fake grid

Large — [[server-world-link-sets]].

## Viewer

The link refusal alert; chords via `e2e_live_checks` live.
