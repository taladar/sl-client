---
id: viewer-p14-1
title: Ingest AvatarAppearance
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 14 — Server-published baked texturing (incl. alpha)
---

Context: [context/viewer.md](../context/viewer.md).

**P14.1. Ingest `AvatarAppearance`.** In `avatars.rs`, on
`Event::AvatarAppearance` decode `texture_entry`
(`decode_texture_entry(_, avatar_texture::COUNT)`), read the baked-slot UUIDs
(`avatar_texture::*_BAKED`), and request each through the shared
`TextureManager` / `TextureStore` (the Phase-6 pipeline). Track per-avatar.
(On SL these come from the server "Sunshine" bake; on OpenSim they come from
*other* avatars' viewers' client-side bakes — either way they are published
baked UUIDs we just fetch.)
