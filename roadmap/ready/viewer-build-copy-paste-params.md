---
id: viewer-build-copy-paste-params
title: Build floater — per-tab parameter copy / paste
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-prim-texture-editing,
       viewer-build-undo-redo, viewer-build-tool-modify-permission-gate]
---

Context: [context/viewer.md](../context/viewer.md).

The FS build-clipboard family — the fastest way to make one prim match
another. Small copy/paste button pairs sit on each build-tool block,
several with a menu choosing what to transfer:

- **Position / size / rotation** on the Object tab (`copy_pos_btn`,
  `paste_pos_btn` etc.), each also with a paste-from-**system
  clipboard** variant (`paste_*_clip_btn`) and a "copy all vs single
  aspect" menu (`menu_copy_paste_pos/_size/_rot.xml`).
- Whole **object parameters** (shape) — `copy_params_btn` /
  `paste_params_btn`, `menu_copy_paste_object.xml`.
- **Features** (flexi / light / physics block) — `copy_features_btn` /
  `paste_features_btn`, `menu_copy_paste_features.xml` and
  `menu_copy_paste_light.xml`.
- The Texture tab's **face params** — `copy_face_btn` /
  `paste_face_btn` (`menu_copy_paste_texture.xml`, `_color.xml`): a
  multi-face clipboard carrying textures with full-perm checks,
  per-face maps, transforms, and colours.

We have none of this (grep over `edit_tool.rs` / `edit_params.rs` /
`edit_texture.rs` finds only `derive(Copy)`). Implementation: a typed
clipboard resource per family (a params snapshot), copy from the
current selection, paste committing through the same
MultipleObjectUpdate / ObjectShape / ObjectExtraParams / ObjectImage
spines the spinners already use, greyed without modify permission
([[viewer-build-tool-modify-permission-gate]] done).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml`
(copy_pos_btn…paste_rot_clip_btn L1643-1838, copy_params L1864-1878,
copy_features L2819-2833), `panel_tools_texture.xml` (copy_face_btn
L132-145), `menu_copy_paste_pos.xml` and the seven sibling menus,
`indra/newview/llpanelobject.cpp` (onCopyPos/onPastePos…),
`indra/newview/fspanelface.cpp` (onCopyFaces/onPasteFaces),
`indra/newview/llpanelvolume.cpp`.
