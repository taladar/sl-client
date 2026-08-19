---
id: viewer-build-grid-options
title: Grid-options floater + snap-XY / selection-grid
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-transform-gizmos]
---

Context: [context/viewer.md](../context/viewer.md).

The build grid configuration surface:

- **Grid Options… floater** (`build_options`): grid unit size, extents,
  sub-unit snapping, grid opacity and cross-section display.
- **Snap Object XY to Grid** (Shift+X in the reference): snap the
  selection's horizontal position to the current grid.
- **Use Selection for Grid** (Shift+G): make the current selection
  define the grid frame (origin + orientation), so other objects snap
  relative to it; reset back to world grid.

Scope: the floater, the two commands with shortcuts, and the grid-frame
state (world / local / reference-object) consumed by the gizmo snapping
logic.

Reference (Firestorm, read-only): `menu_viewer.xml` Build ▸ Options
(~L2342-2377), `llfloaterbuildoptions.cpp`, `LLSelectMgr` grid-mode
state.

Builds on: the transform gizmos (blocked task) — grid config and frames
feed their snapping.

## Parity-audit addendum (2026-08-19)

Parity-audit extension: the rotation snap step (`RotationStep`) — the
angular increment used when rotating with snapping, alongside the
task's existing grid-unit/snap options.

The reference grid-options floater (`floater_build_options.xml`) also
carries two controls not yet in this task's body: the **RotationStep**
angle-snap spinner (the rotation equivalent of the grid unit — snap
increment for the rotate gizmo) and the Firestorm **decimal precision**
setting (`FSBuildToolDecimalPrecision`, the number of decimals shown by
the build floater's numeric fields). Both live at
`floater_build_options.xml` L56-72; include them in the floater.
