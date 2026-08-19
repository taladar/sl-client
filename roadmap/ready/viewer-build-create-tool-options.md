---
id: viewer-build-create-tool-options
title: Create tool — remaining shape presets + copy options
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-creation, viewer-create-shift-drag-duplicate,
       viewer-build-creation-defaults]
---

Context: [context/viewer.md](../context/viewer.md).

The reference Create panel offers 13 prim shape presets; ours offers 7
plus tree/grass (`sl-client-bevy-viewer/src/edit_create.rs` keys: box,
cylinder, prism, sphere, torus, tube, ring, tree, grass). Add the
derived presets — ToolPyramid, ToolTetrahedron, ToolCone, ToolHemiCone,
ToolHemiCylinder, ToolHemiSphere — which are parameter presets (cut,
taper, hollow) over the box / cylinder / sphere pcodes.

Also add the four create-tool checkboxes: **Keep Tool selected**
(`CreateToolKeepSelected`), **Copy selection**
(`CreateToolCopySelection` — a click copies the current selection
instead of rezzing a new prim), **Center Copy**
(`CreateToolCopyCenters`) and **Rotate Copy** (`CreateToolCopyRotates`).
Our held-Shift keeps-the-tool gesture (`edit_create.rs`) stays; the
checkboxes persist the modes as settings. Shift-drag duplicate is
separate and done ([[viewer-create-shift-drag-duplicate]]); default
creation parameters for new prims are [[viewer-build-creation-defaults]].

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml`
(ToolPyramid…ToolHemiSphere L469-571, CreateTool* checkboxes L636-671),
`indra/newview/lltoolplacer.cpp`.
