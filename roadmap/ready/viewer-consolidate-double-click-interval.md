---
id: viewer-consolidate-double-click-interval
title: Consolidate the per-widget double-click interval constants
topic: viewer
status: ready
origin: user request (2026-07-27), while adding the avatar-profile group list
refs: [viewer-social-groups, viewer-avatar-profile-group-list]
---

Context: [context/viewer.md](../context/viewer.md).

Many lists and surfaces have grown their **own** double-click-interval
constant (audited 2026-10-15) — mostly 0.4 s, but inventory already drifted to
0.35 s, which is exactly the inconsistency this task removes:

- `sl-viewer-people/src/groups.rs` `DOUBLE_CLICK_SECS` (People-pane groups).
- `sl-viewer-people/src/avatar_profile.rs` `GROUP_DOUBLE_CLICK_SECS` (the
  2nd-Life group list).
- `sl-viewer-people/src/people.rs` `DOUBLE_CLICK_SECS` (friends list).
- `sl-viewer-people/src/radar.rs` `DOUBLE_CLICK_SECS` (radar rows).
- `sl-viewer-places/src/top_objects.rs` `DOUBLE_CLICK_SECS` (top objects).
- `sl-viewer-edit/src/edit_contents.rs` `DOUBLE_CLICK_SECONDS` (contents tab).
- `sl-viewer-inventory/src/inventory.rs` and `inventory_gallery.rs`
  `DOUBLE_CLICK_SECS` (0.35 s).
- `sl-viewer-map/src/minimap.rs` `DOUBLE_CLICK_SECONDS` and
  `sl-viewer-map/src/world_map.rs` `WORLD_MAP_DOUBLE_CLICK_SECONDS` (plus a
  6 px slop constant each).
- `sl-client-bevy-viewer/src/double_click_teleport.rs` `DOUBLE_CLICK_SECONDS`
  (in-world double-click, also with a 6 px slop).

(`DOUBLE_TAP_RUN_WINDOW_SECS` in `sl-viewer-world-view/src/movement.rs` is the
double-tap-to-run key window, a different setting — leave it out.)

Replace them with a **single** source of truth: either one shared const in a
common UI module (e.g. `crate::ui`), or — better — a **preference** (user-facing
options belong in the preferences floater, not per-site consts), so the
double-click speed is tunable like a real viewer's. Wire every double-click site
to it.
