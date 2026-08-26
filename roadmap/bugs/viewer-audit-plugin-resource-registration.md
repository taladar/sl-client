---
id: viewer-audit-plugin-resource-registration
title: Two plugins read resources they never register
topic: viewer
status: bugs
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
