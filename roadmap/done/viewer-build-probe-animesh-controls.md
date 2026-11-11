---
id: viewer-build-probe-animesh-controls
title: Features tab — reflection-probe & animated-mesh controls
topic: viewer
status: done
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-realtime-mirrors]
---

Context: [context/viewer.md](../context/viewer.md).

The Features tab's remaining blocks, both absent from
`sl-viewer-edit/src/edit_params.rs`:

The **Reflection Probe** checkbox with its sub-controls — Volume Type
combo (Sphere / Box), Dynamic flag, Update Type combo (Static /
Dynamic / Mirror / Dynamic Mirror), and the Ambiance and Near Clip
spinners. The protocol type already exists (the `ReflectionProbe`
extra-param in `sl-proto/src/types/object.rs`) and we already *render*
probes (P33-2 done; hero mirrors via [[viewer-realtime-mirrors]] in
progress) — we just cannot author them.

The **Animated Mesh** checkbox — the animesh extra-param flag that
turns a rigged-mesh linkset into a control-avatar. We render animesh
(`sl-viewer-world-avatar/src/animesh.rs`) but cannot set the flag on an
object.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L2796,
3062-3155), `indra/newview/llpanelvolume.cpp`.

## Done (2026-11-10)

Both blocks are on the Features tab (`sl-viewer-edit/src/edit_params.rs`,
their rules in `edit_probe.rs`):

- **Animated Mesh** first, above the per-prim features, as the reference
  orders it. Live for one selected linkset whose root is the primary
  (`ObjectState::heads_linkset`), within the reference's
  `canBeAnimatedObject` — the linkset's summed mesh triangle estimate
  (`MeshHeader::est_triangles_max`, the estimator now shared with the
  render-cost model in `sl-mesh/src/cost.rs`) at most the region's
  `AnimatedObjectMaxTris` — and, for a worn linkset not animated yet, room
  under the attachment cap (the account's `animated_object_limit`, or the
  region's `MaxAgentAnimatedObjectAttachments` where the login sent no
  benefits package, Firestorm's OpenSim fallback). The toggle flips
  `ANIMATED_MESH_ENABLED_FLAG` in the root's `ExtendedMesh` block, which stays
  in use with the bit cleared.
- **Reflection Probe**: ticking it raises `ReflectionProbeApplied` and, on OK,
  sends the probe block (a box for a prim on a linear path, Firestorm's
  choice; a sphere otherwise) and makes the volume match it — phantom, the
  PBR material cleared off every face (only where one is on), every face
  transparent, a fresh box / sphere shape. The Volume combo, the Dynamic
  checkbox (shown while realtime mirrors are off) / Update Type combo
  (Static / Dynamic / Mirror (Environment) / Mirror (Everything), shown while
  they are on), and the Ambiance / Near Clip spinners follow; a mirror is
  always a box and greys the volume type and spinners. A volume change that
  flips the box bit reshapes the prim again, a sphere taking its X size on
  every axis. Unticking drops the block and, in a linkset, the phantom flag.

Found on the way: the build floater's local echo of an edit made the
simulator's echo of it compare equal to the tracked object, so the scene
never refreshed its components — a retuned light, a probe made a mirror or
a prim made physical changed in the panel only. `TrackedObject::local_echo`
now marks the echo and the change gate refreshes on the next update.

Tests: unit tests for the probe rules, the animesh gate, the reshape sends,
the combos and the row swap; the fake-grid e2e stage
`a_prim_is_made_a_mirror_and_back_on_the_fake_grid` makes a prim a mirror
through the UI and reads the grid's record and the viewer's new `mirrors`
state probe (`Probe::Mirrors`, `sl-viewer-ctl mirrors`); the aditi stage
`a_mirror_reflects_the_avatar_on_second_life` builds a real mirror with it
([[viewer-realtime-mirrors]]).
