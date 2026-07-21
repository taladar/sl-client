---
id: viewer-land-context-menu
title: Land / terrain context pie menu entries
topic: viewer
status: ready
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

Wire now: **Sit Here** (the ground-sit command exists in the self avatar
pie); **About Land** if/when a parcel info surface exists (parcel data is
already modelled). The rest — build/terraform, buy flows, go-here
autopilot, particle-owner mute — start greyed and go live as their features
land.

Picking: the terrain is already ray-testable (the `ground.rs` probe raycasts
the rendered terrain); the missing piece is only routing a right-click that
hits neither UI, HUD, avatar ([[viewer-avatar-mesh-accurate-pick]]) nor an
object to the land pie.

**Pin every entry's position** (the [[viewer-ui-radial-menu]]
angular-stability rule): ship the committed address-table test
(`…keeps_every_address`) in the same commit, as the avatar pies do.

Follow-up: [[viewer-land-menu-reorder-when-implemented]] re-lays the pie by
meaning once most entries are real.

Reference (Firestorm, read-only): `menu_pie_land.xml`,
`lltoolpie.cpp` (`gMenuLand` / pie dispatch), `llviewermenu.cpp` (handlers).
