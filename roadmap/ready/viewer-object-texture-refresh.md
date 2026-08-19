---
id: viewer-object-texture-refresh
title: Object / attachment texture refresh (Object.TexRefresh)
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-object-context-menu, viewer-attachment-context-menu,
  viewer-derender-blacklist, viewer-asset-failure-edge-retry]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's Texture Refresh verb — Reset ▸ Tex Refresh on the object
pie and Tex Refresh on both attachment pies (`Object.TexRefresh`,
`handle_object_tex_refresh`) — re-fetches the picked linkset's textures.
It is the classic "blurry prim" fix and the object-side sibling of the
avatar Tex Refresh we already wired (`RefetchAvatarTextures` in
`avatars.rs`, reachable from `avatar_menu.rs`). Our tree holds
UNIMPLEMENTED placeholders at all three addresses: `tex-refresh` in
`sl-client-bevy-viewer/src/object_menu.rs` (RESET_PIE) and in
`attachment_menu.rs` (self and other pies).

Scope: a per-linkset texture re-fetch — drop the linkset's face textures
from the decoded caches and re-request them from the grid. The
building blocks exist: the derender/unhide re-fetch machinery in
`derender.rs` / `asset_blacklist.rs` ([[viewer-derender-blacklist]]) and
the asset-store invalidation paths touched by
[[viewer-asset-failure-edge-retry]]. Wire the re-fetch to the three
`tex-refresh` slices; faces carrying media re-kick their media texture
as well, matching the reference behaviour.

Reference (Firestorm, read-only): `indra/newview/llviewermenu.cpp`
(`Object.TexRefresh` / `handle_object_tex_refresh`),
`indra/newview/skins/default/xui/en/menu_pie_object.xml`,
`menu_pie_attachment_self.xml`, `menu_pie_attachment_other.xml`.
