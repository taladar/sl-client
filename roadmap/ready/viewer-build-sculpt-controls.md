---
id: viewer-build-sculpt-controls
title: Object tab — sculpt editing controls
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-ui-texture-picker]
---

Context: [context/viewer.md](../context/viewer.md).

The Object tab's sculpt block in the reference: a sculpt-map texture
picker (`texture_picker Default`), the stitching-type combo (`sculpt
type control`: Sphere / Torus / Plane / Cylinder), and the Mirror /
Inside-out checkboxes (`sculpt mirror control` / `sculpt invert
control`); plus offering "Sculpted" in the base-type combo so an
ordinary prim can be *switched* to a sculpt.

Today our parameter tab classifies sculpt/mesh prims read-only and
does not offer editing the sculpt texture — a deviation recorded in
the `sl-client-bevy-viewer/src/edit_params.rs` module doc before the
texture-picker widget shipped; [[viewer-ui-texture-picker]] is done
now, so the blocker is gone. Edits commit as the PARAMS_SCULPT
ObjectExtraParams block the protocol already carries (sculpt type byte
packs the stitching type plus the mirror/invert flag bits).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L2407-2465),
`indra/newview/llpanelobject.cpp`.
