---
id: viewer-audit-parcel-borders-retessellation
title: A region with parcels but no terrain re-tessellates its overlay every frame
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/parcel_borders.rs:625` — when a region has a parcel
grid but no loaded terrain, the `else { continue; }` skips the `stamps.insert`
at `:640` while `pending.remove` has already run at `:615`. So the dirty test
re-marks the region next frame, and its whole 64x64 overlay is re-tessellated
every frame, forever.

The sibling `continue` at `:617` has a comment saying its re-dirty is
intentional; this one inherits that silently at roughly a thousand times the
cost. Either record the stamp on the skip path, or keep the region in `pending`
so the retry is explicit and bounded.
