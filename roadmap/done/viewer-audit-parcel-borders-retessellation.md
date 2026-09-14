---
id: viewer-audit-parcel-borders-retessellation
title: A region with parcels but no terrain re-tessellates its overlay every frame
topic: viewer
status: done
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

## Resolution (2026-09-14)

**A build attempt now stamps on every path, drawn or not.** The two skips were
the same omission: a region with no streamed overlay grid, and a region whose
bands all failed to drape because its terrain had not arrived, both left
`stamps` without an entry — and an unstamped region is dirty by definition, so
the next frame tessellated its whole 64x64 overlay again. The grid and the
per-region terrain revision the stamp records are *exactly* what change when
the missing input arrives, so the retry is change-driven rather than per-frame.

`RegionStamp.grid` is now `Option<ParcelOverlayGrid>` — "the region had no grid
when we tried" is a state the stamp can hold — and the dirty test moved out of
the system into a pure `stamp_is_stale`, which the tests drive directly.

A failed build also despawns any bands an earlier state left for that region
(terrain purged out from under them), instead of leaving geometry draped over
terrain that is gone.

Unit-verified, two tests in `parcel_borders.rs`:
`a_region_that_draws_nothing_is_still_stamped` (grid present, no terrain: no
mesh built, a stamp recorded, `stamp_is_stale` false with nothing changed and
true once `bump_revision` fires, and the retry re-stamps at the new revision)
and `a_region_with_no_grid_is_stamped_and_waits_for_one`. Both panic against
the old code, which never reached the insert. The overlay grid is put in place
by a new `SlParcelOverlay::insert_grid_for_test` (the `_for_test` convention
`ViewerSettings::from_store_for_test` documents — a `cfg(test)` item is not
compiled for a dependent crate's tests).
