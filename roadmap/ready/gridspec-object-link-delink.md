---
id: gridspec-object-link-delink
title: Link order, limits and refusals on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-object-properties]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

First id is the root; OpenSim links same-owner only. SL link order ("the prim
linked last is 2") and limits are unmeasured.

Measured with the record ([[gridspec-object-properties]], 2026-10-09): a link
sends the linker the record of each child on Second Life and the root's on
OpenSim; a child's record carries the root's permission masks on both and the
root's sale state on OpenSim only; a select of the root answers for the root
alone.

## Discover

Run `object-link-delink` on aditi; probes for link order, too-far / too-many
prims and their refusal, delink of the root.

## Document

`book/src/gridspec/building.md` § Linking.

## Fake grid

Large — [[server-world-link-sets]].

## Viewer

The link refusal alert; chords via `e2e_live_checks` live.
