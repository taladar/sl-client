---
id: server-world-changed-object-crossing
title: Measure and raise changed() when an unworn object crosses a region border
topic: server
status: ready
origin: server-world-changed-raisers (2026-11-03), left unmeasured
refs: [server-world-changed-raisers, fake-grid-own-attachments-and-region-moves,
  server-physics-integration, viewer-object-moved-region-reuse]
blocked_by: [gridspec-neighbours-crossing]
---

Context: [context/server.md](../context/server.md).

## What

[[server-world-changed-raisers]] measured `CHANGED_REGION` and
`CHANGED_TELEPORT` only for an object worn across a teleport (handed to
[[fake-grid-own-attachments-and-region-moves]]). An object that is not worn
and crosses into the neighbouring region by itself has not been measured. The
fake grid cannot make that crossing either: `sl-fake-grid` has neighbouring
regions and an avatar crossing (`fixtures/border.rs`, `crossing.rs`), but no
object ever moves from one region to the next.

## Scope

- **Measure.** Add a probe (for example `events/changed-crossing`) whose
  object crosses the border by itself. Try the ways a script or a viewer can
  do that, and record which work on which grid:
  - a script's `llSetPos` / `llSetRegionPos` past the edge;
  - a viewer's edit move past the edge;
  - a physical object pushed over it.

  For each, record the bits, how many times they come, in which prims, and
  whether the script carries on, restarts (`state_entry` again) or keeps its
  globals and listens. Run it on aditi (the border rez spot from the
  "Aditi border rez & camera" notes) and both local OpenSim engines; the
  local 2×2 megaregion may hide a crossing, so check that first and use a
  region with a real neighbour if it does.
- **Write it down** in `book/src/gridspec/lsl.md`, in the owner-and-region
  table of § `changed`: what a viewer's change raises.
- **Cross on the fake grid.** Move an object, its contents and its running
  scripts (saved and restored as a take and a rez already are) from one
  region's store into the neighbour's, retire it on the old region's
  circuits and announce it on the new one. Raise what each grid raises. Hold
  it with a `tests/scripts.rs` case on both flavours. Physical motion over the
  edge waits for [[server-physics-integration]]; a script's or a viewer's
  move does not.
