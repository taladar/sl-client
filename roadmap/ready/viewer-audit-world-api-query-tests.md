---
id: viewer-audit-world-api-query-tests
title: sl-viewer-world-api has 214 functions and 5 tests
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-object-children-index]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-api` is the largest test gap in the viewer: 214 functions and
methods against **5 tests**, all of them on `TerrainState` and small helpers.

Every one of these is pure over a synthetic `HashMap` and needs no Bevy — and
none is referenced from any test anywhere in the workspace:

- `ObjectState::{tracked_descendants:4874, linkset_members:4992,
  linkset_root_of, minimap_objects:5338, attachment_roots_by_wearer:5402,
  pick_summary, non_motion_blocks_changed}`. `non_motion_blocks_changed` is the
  highest-consequence one — it decides whether a re-tessellation happens;
- the extra-param parsers `reflection_probe_from_object:5692`,
  `particles_from_object:5837`, `bevy_rotation_of:5936`,
  `surface_info_from_hit:6238`.

Four self-contained state machines in the same file, also untested and also
plain data-in/data-out: `DerenderList` (`:2686-3029`, with `HiddenBy` /
`DerenderKind`), `SelectionSet` (`:109-357`, add/remove/toggle/linkset
semantics), `MuteModel` (`:632`) and `CameraRig` (`:2263`).

This is the prerequisite for [[viewer-audit-object-children-index]] — the
children index changes exactly these query methods, and there is nothing pinning
their current behaviour.
