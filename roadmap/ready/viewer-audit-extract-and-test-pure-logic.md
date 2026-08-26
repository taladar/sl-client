---
id: viewer-audit-extract-and-test-pure-logic
title: Extract the pure logic trapped inside viewer systems and test it
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [viewer-audit-collider-settle-treadmill]
---

Context: [context/viewer.md](../context/viewer.md).

A collection of small, high-value extractions. Each is plain data in, plain data
out, currently zero-coverage, and several would have caught a bug in this audit.

**Physics and camera** (`sl-viewer-world-view`):

- `collider_needs_build(existing, scale, non_solid, shape) -> bool` and
  `collider_job_settled(mesh, physics_available, points_empty) -> (bool, bool)`
  from `physics.rs:2096-2158` — one assertion fails today, see
  [[viewer-audit-collider-settle-treadmill]];
- `collide_camera` (`camera.rs:1326`) is **already** a free function over
  `(&StaticRaycastIndex, &DynamicColliders, focus, eye, &HashSet<Entity>)` and
  has no test, despite governing the most user-visible camera bug class. Assert:
  no hit leaves the eye unchanged; a wall at `d` puts it at
  `d - COLLISION_PADDING`; **a focus inside a cuboid gives the far exit, not the
  origin** (the hollow-cast rationale the comment spends 12 lines on); the
  ignore-set excludes own-avatar colliders;
- `scroll_notches` (`camera.rs:386`) — assert a 20 px `Pixel`-unit scroll equals
  one `Line` notch, the platform-parity guarantee the constant exists for;
- `avatar_at_ground_floor` (`physics.rs:2424`), `clip_axis` (`:759`),
  `collider_extents` (`:456`);
- `gather_object_geometry` (`physics.rs:1512`) with a small ECS app: assert
  linkset children are excluded and holder scale is applied **exactly once** —
  the invariant a physical flexi or tree currently violates, because
  `refine_physical_colliders` (`:1600`) lacks the flexi/category guards its
  static twin has at `:2023`;
- `parent_local_point` (`gizmos.rs:2540`) and `parent_local_rotation` (`:2557`)
  are correct only by cancellation, relying on the ancestor chain carrying
  exactly one `sl_to_bevy_rotation` at the root. Nothing enforces it; one extra
  Bevy-only rotation between root and child silently corrupts every linked-part
  drag.

**Edit tools** (`sl-viewer-edit`): `clamp_corner_factor` (`gizmos.rs:2314` —
assert no object leaves `[MIN_PRIM_SCALE, MAX_PRIM_SCALE]` and that one
saturating object clamps the whole selection), `primary_face_fold` (`:2295`),
`off_line_distance` (`:2335`), `corner_scale_ticks` (`:689`),
`face_scale_ticks` (`:630`), and the three pure functions in
`edit_texture_align.rs`. `node_modifiable` / `node_movable`
(`edit_undo.rs:69`, `:77`) are tested only with `properties() == None`, so the
actual permission-mask decode is uncovered.

**Feature crates**: `InventoryFilter::passes` (`inventory_filters.rs`, 861 lines
/ 4 tests — the single most testable predicate in the group),
`Page::set_results` (`search.rs:640`), `WorldMapTiles::{request,drain,state}`,
`tile_level` / `tile_corner`, the parcel-audio autoplay decision, `expiry_text`
/ `parcel_owner_label` / `day_cycle_summary` (`about_land.rs`, 3309 lines / 0
tests), `prettify` and the `wearable_permissions` to-text round-trip
(`edit_wearable.rs`).

**UI**: `apply_persisted_widths` / `encode_widths` (`ui_table.rs:1315`, `:1339`
— a string round-trip with clamping), `arrow_scroll_delta` (`ui_tab.rs:836`),
`rank_local_lights` extracted from `drive_local_lights`
(`sl-viewer-world-scene/src/lights.rs:264` — the existing tests cover only
`luminance` and `legacy_distance_attenuation`, not the ranking, the
zero-brightness skip, or the `MAX_LOCAL_LIGHTS` truncation), `menu.rs`'s
`navigable_rows` / `step_highlight`, and
`ScreenshotSchedule::step` extracted from `screenshot.rs:125-153`
(assert the first capture lands at exactly `start_delay`, `next_at` advances by
exactly `interval`, exactly `max_frames` captures, and `HoldForSaves` rather
than `Finish` while saves pend — plus that `ScreenshotSchedule::new` (`:74`)
rejects a non-numeric env value instead of dumping every frame).

**gpu_pick render invariants** (`gpu_pick/render.rs`, zero tests) — all
reachable through the pure `specialize(&self, key, layout)` (`:86`): assert
`Err` for `GpuPickKey { skinned: true }` over a POSITION-only layout;
`depth_compare == GreaterEqual` (`:134`, where a `Less` silently picks the
**farthest** surface with no visual tell); `cull_mode == None` (`:128`);
`targets[0].format == Rgba32Uint` (`:122`, which must match
`parse_centre_pixel`).

**The viewer binary's own pure functions**, all untested: `parse_sl_vec3`
(`lib.rs:686` — the SL-to-Bevy `(x,y,z)` to `(x,z,-y)` axis map, silently wrong
if the flip ever regresses), `grid_login_uri` (`:701`), `resolve_login_uri`
(`:717` — a four-level precedence ladder, duplicated verbatim in both REPL
binaries), `replay_camera_start` (`:2586`).
