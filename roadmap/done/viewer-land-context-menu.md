---
id: viewer-land-context-menu
title: Land / terrain context pie menu entries
topic: viewer
status: done
origin: user request while reviewing the pie-menu cluster (2026-07-21)
blocked_by: [viewer-ui-radial-menu]
refs: [viewer-object-context-menu, viewer-avatar-context-menu, viewer-land-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

The pie menu offered when **bare terrain / land** is the pick target
(right-click on the ground), following the pattern
[[viewer-avatar-context-menu]] established: reproduce the **reference's**
entry set at the reference compass positions, declare not-yet-implemented
entries greyed out (the `UNIMPLEMENTED` condition pattern in
`src/avatar_menu.rs`), and wire the entries whose wire path already exists.

Reference entry set (`menu_pie_land.xml` — the pie XMLs are shared by every
skin, the Vintage skin overrides none of them, so `default/xui/en/` is
authoritative): **About Land…**, **Sit Here** (ground sit — the wire path
exists), **Buy This Land**, **Buy Pass**, **Edit Terrain**, **Create**
(build), **Go Here**, **Mute Part. Own.** (mute the particle owner).

Reference (Firestorm, read-only): `menu_pie_land.xml`,
`lltoolpie.cpp` (`gMenuLand` / pie dispatch), `llviewermenu.cpp` (handlers).

## Outcome (2026-07-22)

Implemented as `src/land_menu.rs`, the fourth entry module over the shared
radial widget. The reference's eight slices sit at their XML slots (East →
SouthEast): About Land…, Create, Go Here, Sit Here, Mute Part. Own., Buy
Pass, Edit Terrain, Buy This Land. The committed address-table test
(`land_pie_keeps_every_address`) pins every position.

Wired: **Sit Here** → `Command::SitOnGround`, standing an object-seated
avatar up first as the reference's `LLLandSit` does. Where the reference
then *autopilots* to the clicked point and sits there, ours sits in place —
autopilot does not exist yet (Go Here is the same gap); upgrade both
together. Everything else is greyed via `UNIMPLEMENTED`: About Land waits
for a parcel-info surface ([[viewer-parcel-options-general]]), Create /
Edit Terrain for build + terraform tools, Go Here for autopilot, Mute
Part. Own. for particle picking ([[viewer-particle-pick-mute]]), the two
buy slices for the land-buy flows.

Routing: the shared right-click resolver in `avatar_menu.rs` now also
resolves the world ray against the `TerrainSurface` patches (first-hit-only,
so terrain never picks through an object or avatar body); terrain competes
with the occlusion-blind avatar pick by distance, avatar winning ties.

Follow-up: [[viewer-land-menu-reorder-when-implemented]] re-lays the pie by
meaning once most entries are real.
