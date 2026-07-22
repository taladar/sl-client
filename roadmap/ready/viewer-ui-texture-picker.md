---
id: viewer-ui-texture-picker
title: Texture picker floater — choose a texture from inventory
topic: viewer
status: ready
origin: user request (2026-07-22), noticed while live-testing
  viewer-social-profiles (profile pictures cannot be set)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-texture-preview-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **texture picker** (`LLFloaterTexturePicker`, opened by every
`texture_picker` / `LLTextureCtrl` widget): a floater listing the inventory's
textures / snapshots with a search filter, a preview pane, and **None** /
**Blank** / **Default** quick choices, returning the chosen `TextureKey` to
the widget that opened it. Local-file textures are the follow-up task
[[viewer-local-textures]] (the picker's "Local" tab), and the bake channels
can wait; inventory selection + preview + None/Blank is the useful core.

Ship it as a reusable widget: an `OpenTexturePicker { requester, current }`
message and a `TexturePicked { requester, texture }` reply, so any panel can
host a swatch that opens the picker. The inventory model already carries the
texture items; the preview path is the item-preview decode
(`inventory_properties.rs`) / [[viewer-texture-preview-floater]].

Consumers waiting on it: [[viewer-profile-image-editing]] (profile / 1st-life
pictures, pick and classified snapshots), later the build tool's face
textures, terrain textures, group insignia.

Reference (Firestorm, read-only): `llfloatertexturepicker.cpp`,
`lltexturectrl.cpp`, `floater_texture_ctrl.xml`.
