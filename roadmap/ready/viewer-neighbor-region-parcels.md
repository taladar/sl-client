---
id: viewer-neighbor-region-parcels
title: Neighbour-region parcel tracking (About Land on neighbours)
topic: viewer
status: ready
origin: About Land floater follow-up (2026-07-27)
refs: [viewer-parcel-options-general, viewer-parcel-options-access-media]
---

Context: [context/viewer.md](../context/viewer.md).

The library only tracks the **current** region's parcels, so any parcel UI
(About Land, the parcel pie, join/split) can only act on the region the agent
stands in. A right-click on a **neighbour** region's terrain has no parcel to
resolve to — the About Land pie slice falls back to the agent's own parcel.

Two library-side gaps in `sl-client-bevy`'s `world.rs` cause this:

- **`upsert_parcel` attributes every `ParcelProperties` to `SlCurrentRegion`**
  (it early-returns on `index.current` and keys parcels by the current handle),
  so neighbour-circuit `ParcelProperties` are misfiled under the current region
  and no neighbour region ever gains `SlParcel` children. It should attribute a
  reply to the region of the **circuit it arrived on** (like the parcel-overlay
  ingest already does via `ParcelOverlayInfo::region_handle`).
- **`SlRegion` carries no child-circuit id**, so even with per-region parcels
  there is no `CircuitId` to build the [`ScopedParcelId`] a parcel command
  needs for a neighbour. Track each region entity's circuit id (root and child)
  so a consumer can scope a command to the right circuit.

With those in place, extend the About Land floater
([[viewer-parcel-options-general]]) to key its subject on
`(region_handle, local_id)` rather than a bare `local_id`: resolve the pie's
clicked point to the neighbour region it fell in, read that region's
`SlParcel` / `SlRegionIdentity` (type, rating) for the General and Covenant
tabs, and scope its `RequestParcelObjectOwners` / `UpdateParcel` to that
region's circuit. Note the covenant is **estate**-scoped, so a neighbour in a
different estate needs its own `EstateCovenant` (the current
`RequestEstateCovenant` only covers the region the agent occupies).

Reference (Firestorm, read-only): `llviewerparcelmgr` (per-region parcel
selection), `llviewerregion` (each region owns its parcel manager).

Deps: none hard — the library change is independent of the floater; the floater
extension builds on it. Related: [[viewer-parcel-options-general]],
[[viewer-parcel-join-split]].
