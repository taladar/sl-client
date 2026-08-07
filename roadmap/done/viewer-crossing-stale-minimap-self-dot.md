---
id: viewer-crossing-stale-minimap-self-dot
title: Stale own-avatar dot left in the old region on the minimap after a crossing
topic: viewer
status: done
origin: user report (2026-08-07), teleport/crossing live testing
refs: [viewer-minimap, viewer-seamless-region-handover-objects]
---

Context: [context/viewer.md](../context/viewer.md).

After walking/flying across a region border, the minimap left a **stale avatar
dot in the old region** — the coarse-location dot for the *own* avatar (or a
neighbour dot) was not cleared/moved when the root region changed.

The minimap draws coarse-location dots offset by each region's global metres
relative to the current origin (`avatars.rs` coarse translation ~2407). On a
crossing the old region's `CoarseLocationUpdate` entry (and its dot) should be
dropped or superseded by the new region's; instead a ghost dot lingers.

Investigate: on a root-region change, prune coarse-location entries for the
region left behind (or reconcile them against the new `CoarseLocationUpdate`),
so no stale self/neighbour dot remains. Related to the broader
[[viewer-seamless-region-handover-objects]] (world state not reconciled on a
handover), but scoped to the minimap coarse dots.

Reference (Firestorm, read-only): `LLWorld` coarse-location handling per region,
`llnetmap` dot sourcing.
