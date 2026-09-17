---
id: viewer-neighbour-object-caps-use-root-region
title: Per-object capability requests ask the root region about a neighbour's objects
topic: viewer
status: bugs
origin: hover-tooltip neighbour-region "Loading…" investigation (2026-09-17)
refs: [viewer-hover-tooltip-202ms-frame-spike]
---

Context: [context/viewer.md](../context/viewer.md).

Both runtimes answer `Command::RequestObjectCost` (the hover tooltip's land
impact), `RequestSelectedCost` and `RequestObjectPhysicsData` by POSTing to
the **root** region's capability (`sl-client-bevy/src/lib.rs`,
`sl-client-tokio/src/lib.rs`). An object in a **neighbour** region is unknown
to the root simulator, so the land-impact line of a neighbour object's tooltip
has nothing to resolve to.

The reference groups the stale objects by `getRegion()` and asks each
region's own `GetObjectCost` capability (`llviewerobjectlist.cpp`
`fetchObjectCosts`; the commented-out `gAgent.getRegion()` variant above it
is the old, wrong form).

## What it needs

- The neighbour's capability map: `Event::NeighborSeed` is already POSTed by
  both runtimes (it unlocks the neighbour's object stream), but the map that
  comes back is not kept per region for later requests.
- The command (or the driver) must know each object's region, so a batch is
  split per region and each part goes to that region's capability — the UDP
  half of this, `Session::circuit_for_object`, landed with
  [[viewer-hover-tooltip-202ms-frame-spike]].

## Verify

On aditi (neighbouring regions), hover an object across the border: the
tooltip's `Land Impact` resolves to a number instead of staying `…`.
