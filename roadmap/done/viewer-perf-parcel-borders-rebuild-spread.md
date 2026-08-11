---
id: viewer-perf-parcel-borders-rebuild-spread
title: Parcel borders — rebuild only the changed region, spread new regions
topic: viewer
status: done
origin: async shadow-cull frame decomposition on aditi (2026-08-11)
refs: [viewer-perf-frame-churn-cleanups, viewer-perf-update-objects-budget-moves-removes]
---

Context: [context/viewer.md](../context/viewer.md).

`update_parcel_borders` (`parcel_borders.rs`) rebuilds too much, too coarsely.
The current trigger (`:544`) sets `rebuild_pending` on **any** of
`overlay.is_changed()`, `water.is_changed()`, a terrain-revision bump, or an
origin change — all of which fire repeatedly during streaming even when no
region's parcel **layout** actually changed. And when it does fire it
`despawn_all` + rebuilds **every** loaded region's bands in one frame (`:556`,
`:561`).

Measured on aditi: mean 1.85 ms but **p99 31 ms, max 44 ms** — a recurring
`Update` hitch, because a cooldown bounds only the *frequency*, not the coarse
trigger or the all-regions per-rebuild cost.

A region's border bands only actually change when:

1. **a new region appears** (or the origin shifts, re-placing existing regions);
2. **a parcel is created / joined / subdivided on that region** — its parcel
   grid changed; or
3. **that region is terraformed, or its water height changes** — the bands
   ground-/water-follow, so an edit to the region's terrain relief or sea level
   re-shapes their geometry even when the parcel *layout* is unchanged.

So make it change-driven per region, not a periodic all-regions refresh:

- Keep a **per-region stamp** covering everything a rebuild depends on: that
  region's parcel-overlay grid, its terrain relief, and its water height. Each
  frame, rebuild **only** the regions whose stamp changed or that are newly
  present; despawn only regions that left. No global `despawn_all`, no rebuild
  of unchanged regions. This drops the steady-state cost to ~zero (nothing
  changed → nothing rebuilt), the same "unchanged ⇒ no work" shape as the shadow
  cull and [[viewer-perf-update-objects-budget-moves-removes]].
  - Wrinkle: the terrain revision read today (`terrain.map_revision()`) is
    **global**, so it can't distinguish which region was terraformed — a naive
    per-region stamp keyed on it would still rebuild all regions on any edit.
    The stamp needs a **per-region** terrain/water signature (hash the region's
    height patch + its water height), or a per-region terrain revision, so a
    terraform on one region dirties only that region.
- When several regions do need building at once (e.g. first enable, or crossing
  into a fresh area so multiple neighbours appear together), **spread** those
  region builds over a few frames with a small per-frame budget rather than all
  in one — a property line appearing a few frames late is invisible.
- Drop the blanket `overlay.is_changed()` / `water.is_changed()` /
  terrain-revision triggers as the rebuild gate; fold water height / terrain
  relief into the per-region stamp so a genuine change to *that* region still
  re-tessellates it, but unrelated churn does not.

Acceptance: parked on a rezzed region with a static parcel layout, the overlay
does **zero** rebuilds (Tracy `-f update_parcel_borders` shows only cheap
no-op frames); subdividing/joining a parcel rebuilds only that region;
crossing into an area with several new regions spreads their builds over a few
frames with no >20 ms frame.

## Implemented (2026-08-12)

Change-driven per-region rebuild landed. `TerrainState` gained a per-region
revision (`region_revision`, bumped per region on patch / handshake ingest,
alongside the global `map_revision`). `update_parcel_borders` now keeps a
per-region stamp (parcel-overlay grid + water height bits + terrain revision),
rebuilds only regions whose stamp changed or that are newly present, despawns
regions that left, updates transforms in place on an origin shift, and spreads
rebuilds `PARCEL_REBUILD_BUDGET` (2) regions/frame. The blanket
`overlay/water/revision` gate and the cooldown are gone.

Verified on aditi (Tracy): steady-state `update_parcel_borders` mean
**0.069 ms** (was ~1.85 ms), **zero rebuilds** on most frames, and the recurring
**44 ms all-regions spike is gone** (only rare per-region rebuilds during
terrain streaming remain; a single region's build is ~7-11 ms, so budget could
drop to 1/frame to shave the rare rez-time frame). Borders render correctly and
toggle cleanly (World ▸ Property Lines). Unit test covers the per-region
revision. The finer-grained band-segment spread remains a future option.
