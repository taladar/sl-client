---
id: viewer-audit-plugin-resource-registration
title: Two plugins read resources they never register
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-plugins-own-their-schedule]
---

Context: [context/viewer.md](../context/viewer.md).

- `sl-viewer-world-view/src/camera.rs:404` — `CameraPlugin` inits `CameraMode`,
  `FocusTarget`, `CameraAim`, `CameraTuning` and `FlycamSmoothing` but **not**
  `CameraSpin`, which `drive_flycam` takes as `Res<CameraSpin>`; only the binary
  inserts it (`sl-client-bevy-viewer/src/lib.rs:1785`). Same for
  `Res<SpacenavInput>` in `switch_camera_mode` — the tell is that the existing
  test has to `init_resource::<SpacenavInput>()` by hand (`camera.rs:1761`).
- `sl-viewer-world-objects/src/material_cache.rs:319` and `materials.rs:1084`
  take `Res<SelectionSet>`, but the only `init_resource::<SelectionSet>()` in
  the workspace is `sl-viewer-edit/src/edit_selection.rs:280`, and
  `sl-viewer-edit` is **not** a dependency of `sl-viewer-world-objects`. Any
  host adding the object layer without the edit layer panics.
- `sl-viewer-world-scene/src/parcel_borders.rs:529` takes `Res<WaterState>`,
  which is `insert_resource`d in `setup_water` (`water.rs:243`) — while
  `water_exclusion.rs:293` defensively takes `Option<Res<WaterState>>` for the
  same resource.

Each plugin should `init_resource` what its systems read, or take the resource
as `Option<Res<_>>` where absence is legitimate. The root cause is
[[viewer-audit-plugins-own-their-schedule]].

## Fixed (2026-09-13)

The rule applied is the one the audit proposes, with a line drawn through it:
a plugin registers a resource its systems read when a `Default` is a truthful
stand-in — the resource is the plugin's own, or it is configuration whose
default *is* the "nothing attached / factory value" state. It keeps taking the
resource optionally where absence is a legitimate scene state, and it still
leaves genuine world/session state (`SlIdentity`, `AvatarState`,
`ObjectState`, `InputContext`) to the owners that fill it — a default there
would be a lie, and lying is worse than the panic.

`init_resource` is idempotent, so none of this takes ownership away from the
plugin that already had it: where both are in the app, the owner's
registration still runs first and wins.

**The camera.** `CameraPlugin` now inits `CameraSpin` (its own module's
resource; the viewer's `--camera-spin` only ever *overrode* it) plus
`SpacenavInput` **and `FlycamAxisSettings`** — the audit named only the first,
but `drive_flycam` reads both, so registering one alone would have left the
same panic one parameter along. Fixing it let the two fixture worlds
(`world_test.rs`, `full_stack_test.rs`) drop the `CameraSpin` they each
inserted by hand.

**Avatar movement.** Not in the audit, the same defect: `AvatarMovementPlugin`
read `MovementTuning` — declared in its own `movement.rs`, registered only by
`sl-viewer-preferences` and the binary — and the SpaceNavigator trio
(`SpacenavInput`, `AvatarAxisSettings`, `AvatarNavSmoothing`). All four are
registered there now. Every default is the reference behaviour, which is what
makes this safe: `MovementTuning::default` is today's constants, and a
defaulted `SpacenavInput` is a centred, un-pressed device.

**The selection.** `SelectionSet` moved to `sl-viewer-world-api` since the
audit, so the world layer *can* register it without depending on the edit
layer — which is what `ViewerWorldPlugins` does now, for the two systems it
schedules out of `sl-viewer-world-objects` (`detach_shared_face_materials`,
`apply_blinn_phong_hide`). `DerenderPlugin` needed the same: a derender drops
its target from the selection, and the avatar layer does not depend on the edit
layer either. The fixture world's hand-inserted `SelectionSet` went with it.

**The water.** `update_parcel_borders` takes `Option<Res<WaterState>>`, like
`water_exclusion` and `water_scene_depth` already did — absent, every region
reads as "sea level not learned yet", which the system already handles (it is
the state before a region's handshake). `regen_minimap_layers` had the identical
read and is converted with it; its two hardcoded `20.0` fallbacks are now
`water::DEFAULT_WATER_HEIGHT`, widened from `pub(crate)` to `pub` so the one
grid default has one home.

## How it was verified

Five unit tests, each pinning one of the fixes:

- `sl-viewer-world-view/src/camera.rs` —
  `the_plugin_registers_the_resources_its_systems_read`: `CameraPlugin` alone in
  a bare `App` leaves `CameraSpin`, `SpacenavInput` and `FlycamAxisSettings`
  present.
- `sl-viewer-world-view/src/movement.rs` —
  `the_plugin_registers_the_resources_its_driver_reads`, the same for
  `AvatarMovementPlugin`'s four.
- `sl-viewer-world-avatar/src/derender.rs` —
  `the_plugin_registers_the_selection_it_writes`.
- `sl-viewer-world-scene/src/parcel_borders.rs` —
  `the_bands_run_without_a_water_surface` runs the system for a frame with no
  `WaterState` in the app and one region present, so the stamp comparison really
  evaluates a water height rather than returning before it. Bevy validates a
  system's parameters before its body, so this is the panic itself, not a proxy
  for it.
- `sl-viewer-map/src/minimap.rs` — `water_height_falls_back_to_the_grid_default`
  pins the minimap's fallback.

Plus the two fixture worlds, which now build their apps without the three
hand-inserted resources this fix retires.
