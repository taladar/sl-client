---
id: viewer-texture-drag-drop
title: Drag & drop a texture onto the build Texture tab / an object face
topic: viewer
status: ready
origin: user request (2026-07-24) while reviewing the build-tool texture editor
blocked_by: [viewer-prim-texture-editing, viewer-ui-texture-picker]
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer lets you drag a texture from inventory and drop it
**onto the build floater's Texture section** (the diffuse swatch) or **directly
onto an object face in-world**, applying it without opening the picker
(`LLToolDragAndDrop::dad3dTextureObject` / the texture-ctrl drop target).

Wire it into the existing inventory drag system
([[viewer-inventory-context-actions]], `inventory_drag.rs`): the drag already
carries the dragged `ItemInfo` and ends in `on_row_drag_end`, which resolves
what is under the cursor. Add two drop paths for a dragged **texture /
snapshot** item:

- **Over the Texture-tab swatch / section** (a new UI drop target on the
  build-tool texture swatch): apply the texture to the current face selection —
  emit the same `TexturePicked { requester, texture }` the picker does
  ([[viewer-ui-texture-picker]]), or apply directly through the Texture tab's
  `apply_to_selection` spine.
- **Over an object face in-world**: world-pick the face under the drop
  (`ObjectPicker` + `SurfaceInfo.face_index`, as the Select Face tool does) and
  apply the texture to just that face — matching the reference's per-face
  drop (Shift/Ctrl for all-faces is a later refinement).

Reference (Firestorm, read-only): `lltooldragdrop.cpp`
(`dad3dTextureObject`), `lltexturectrl.cpp` (the ctrl's drop handler).
