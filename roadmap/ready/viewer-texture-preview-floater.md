---
id: viewer-texture-preview-floater
title: Texture preview floater — full reference feature set
topic: viewer
status: ready
origin: user request (2026-07-22), noticed while reviewing the minimal
  Open preview shipped with viewer-inventory-open-and-properties
refs: [viewer-inventory-open-and-properties]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **texture / snapshot preview** beyond the fixed-size
image the inventory Open shipped: the decoded **dimensions** read-out,
**aspect-ratio presets** (the combo of 1:1 / 4:3 / … the reference uses
for judging profile / parcel images), resize-with-the-floater display,
the **Copy UUID** button (full-perm gated, sharing the context menu's
`can-copy-uuid`), and **Save As** — decode to PNG on disk via a file
dialog, which also un-greys the item context menu's "Save As" entry and
the gear menu's "Save Texture As".

Reference (Firestorm, read-only): `llpreviewtexture.cpp`,
`floater_preview_texture.xml`.

Note (2026-08-19): this is the natural home for a **Blacklist this texture**
action. [[viewer-derender-blacklist]] already carries `DerenderKind::Texture`
and honours it at the fetch gate (`TextureManager::request_from` refuses a
blacklisted id outright, as the reference's `LLTextureFetch` does), but
*nothing produces such an entry* — and the reference has no in-viewer producer
either, feeding textures only through its distributed blacklist data
(`fsdata`). A texture surface that already exposes the asset id (Copy UUID
here) is where the action belongs; until then an entry can only be hand-written
into the per-account `derender_blacklist.json`.

Note (2026-07-22): this floater is **subject-bound** — it opens on a
particular subject rather than persistent app state — so exempt it from
floater persistence (`floater_persist::FloaterPersistExempt` on the root,
as the avatar profile and item previews do): no restored rectangle, no
restored "open".
