---
id: viewer-build-texture-tab-fs-extras
title: Texture tab — Firestorm-extended conveniences
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-texture-editing, viewer-edit-face-selection,
       viewer-water-exclusion]
---

Context: [context/viewer.md](../context/viewer.md).

The fspanelface extras beyond the LL texture panel, all absent from
`sl-client-bevy-viewer/src/edit_texture.rs` /
`edit_texture_align.rs`:

- **Flip U / flip V** buttons on the diffuse scale fields
  (`flipTextureScaleU` / `flipTextureScaleV`).
- The **repeats-per-metre** field (`rptctrl`, plus its GLTF/PBR
  counterpart `gltfRptctrl`), converting to raw repeats from the
  prim's face dimensions — we only expose raw repeat U/V.
- The **lock/sync repeats** checkbox (`SyncMaterialSettings`): edits
  to the diffuse transforms mirror onto the normal and specular
  channels.
- The per-channel **"Find All" select-same** buttons
  (`btn_select_same_diff/norm/spec`): extend the face selection to
  every face in the selection using the same diffuse / normal /
  specular map (builds on [[viewer-edit-face-selection]]).
- The **Hide water** checkbox (`checkbox_hide_water`): apply the
  water-exclusion surface to the face — the render side is already
  done ([[viewer-water-exclusion]]), only the authoring checkbox is
  missing.
- The **Align textures** button for the non-planar case (FS's align
  maps across selected faces); the planar variant is done
  (`edit_texture_align.rs`, a full `calcAlignedPlanarTE` port).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_tools_texture.xml`
(L748-795, 267, 1058-1135, 119), `indra/newview/fspanelface.cpp`
(onClickBtnFlipTexture, onClickBtnSelectSameTexture, onAlignTexture,
updateHideWater).
