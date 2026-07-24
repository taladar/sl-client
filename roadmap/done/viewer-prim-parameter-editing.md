---
id: viewer-prim-parameter-editing
title: Prim parameter editing
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The object / features tabs of the edit floater
([[viewer-object-edit-floater-shell]]): name & description,
physics / phantom / temp-on-rez flags, the prim **shape** parameters (path &
profile, cut, hollow, twist, taper, shear, dimple, revolutions), and the
light / flexi / particle feature toggles.

Reference (Firestorm, read-only): `llpanelobject`, `llpanelvolume`; messages
`ObjectShape`, `ObjectExtraParams`, `ObjectFlagUpdate`.

Builds on: `PrimShapeParams` (`sl-proto`), and the existing feature renderers
`flexi.rs`, `lights.rs`, `particles.rs`.

## Done

`sl-client-bevy-viewer/src/edit_params.rs` fills the Build Tools floater's
General, Object and Features pages against the primary selection, with the
reference's tab placement (`floater_tools.xml`): name / description on
**General**, flags + type + shape on **Object**, material + flexi + light on
**Features**:

- **General tab** (the reference's `llpanelpermissions` page): name /
  description (commit → `ObjectName` / `ObjectDescription`, locally echoed
  onto the selection's properties); the read-only creator / owner lines
  (legacy names via the avatar name cache, `UUIDNameRequest`d on demand; a
  group owner via the agent's group list) and a "You can" line from the
  update flags' agent-relative permission bits; the **group** cycle
  (`ObjectGroup`, cycling none → the agent's groups) with a **Deed**
  button (`ObjectOwner` with nil owner — `Command::DeedObjectsToGroup`,
  added to `sl-proto` / both runtimes for this); **Share with group**
  (group mask modify+move+copy as one, the reference's
  `onCommitGroupShare`); **Next owner can** Modify / Copy / Transfer and
  **Anyone** Move / Copy checkboxes (`ObjectPermissions` set/clear, locally
  echoed onto the selection's masks and confirmed by a properties
  re-request). The sale surfaces stay with their own tasks.
- **Object tab**: physical / temporary / phantom toggles
  (`ObjectFlagUpdate`, built from the object's current `PrimFlags` and
  locally echoed); the prim **type** cycle
  (Box / Cylinder / Prism / Sphere / Torus / Tube / Ring, with the
  reference's per-type profile/path curve defaults) and the full shape
  editors — path cut, hollow (%) + hollow-shape cycle, twist begin/end
  (±180° linear / ±360° circular), taper ⁄ hole size, top shear, the
  per-type advanced-cut row (Slice / Dimple / Profile Cut), taper profile,
  radius offset, revolutions, skew. Every shape commit rebuilds the full
  quantized `PrimShapeParams` (the reference's `getVolumeParams` — no
  incremental sends) with the reference's S/T flip (sphere/torus-family
  "Path Cut" edits the *path*, box-family the *profile*), the box-family
  `1 − ratio` taper display (inversion keyed off the previously shown
  type), and the SL clamps (min cut gap 0.02, hollow ≤ 95 %, hole size
  0.05–1 × 0.05–0.5). Sculpt / mesh objects show their type read-only and
  hide the shape rows.
- **Features tab**: the material cycle (`ObjectMaterial`, reference combo
  order, legacy Light excluded); the **Flexible Path** toggle + softness /
  gravity / drag / wind / tension / force editors (enable seeds the
  reference defaults and swaps the path curve LINE ↔ FLEXIBLE with a
  coupled `ObjectShape`, gated on a linear-path plain prim); the **Light**
  toggle + colour (three sRGB fields pending [[viewer-ui-color-picker]],
  linear-byte wire packing with intensity in the alpha byte) / intensity /
  radius / falloff editors, plus FOV / focus / ambiance for an *existing*
  spotlight projection. Feature commits resend the object's **complete**
  `ObjectExtraParams` (the message states the whole set), preserving
  sculpt / animesh / render-material params via the tracked per-object
  extra-params copy (`ObjectState::edit_data`).
- Widgets rewrite only when the underlying object data changes (snapshot
  diffing), so a committed edit is not clobbered while the simulator's echo
  is in flight; flags / material / extra edits are locally echoed
  (`ObjectState::apply_local_*`), while shape deliberately is not (the echo
  must still differ from the fingerprint to trigger re-tessellation).
- With nothing selected the controls stay **visible but greyed out**
  (`ParamGate` per widget → `InteractionDisabled` + pointer-ignore + the
  `.sk-build-disabled` skin class), the reference's `getState` disabling;
  only the per-type visibility hides rows (a sculpt / mesh hides the shape
  rows, a box its taper-profile row), and the flexi / light field groups
  stay visible cleared-and-greyed while their feature is off.
- Particles are not on the reference Features tab; the particle-system
  editor is its own task ([[viewer-particle-editor]]). Sculpt texture /
  type editing waits on [[viewer-ui-texture-picker]]; the type / hollow /
  material combos are cycle buttons pending [[viewer-ui-combo-widget]].
- The floater is **resizable** (a definite content area like the profile
  floater): the tab bar and pages track the window via
  `fill_tab_container`, pages scroll their overflow, and a page wrapper
  inside each container panel carries `UiPanelShown` so a hidden tab's
  fields stay focus-parked.
- Unit tests pin the quantizer inverses, the box/torus display round-trips,
  the reference type-classification table, the type-switch defaults, the
  hollow-shape nibble, the sRGB round-trip, and the flexi / light enable
  defaults.
