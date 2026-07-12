---
id: viewer-p6-1
title: Per-face diffuse
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 6 — Texturing (diffuse only)
---

Context: [context/viewer.md](../context/viewer.md).

**P6.1. Per-face diffuse.** Decode each face's
`TextureEntry.faces[face_id]` (`decode_texture_entry`); request the texture,
convert the decoded RGBA8 with `to_bevy_image`, and build
`StandardMaterial { base_color_texture, base_color = face tint }`. Dedupe
with `HashMap<TextureKey, Handle<Image>>`; faces whose texture has not
arrived use a flat colour from `face.color`. No normal / specular / PBR /
glow / bump. **Done — via the shared `TextureStore`, not inline decode.** On
user direction the viewer drives the LOD-aware `sl_texture::TextureStore`
(the same fetch / off-thread-decode / Firestorm-disk-cache / weak-ref-dedupe
pipeline the headless client uses) rather than decoding J2C on the render
thread. A new `textures.rs` module owns a `TextureManager` resource (store
over a `BevyTextureFetcher` whose `GetTexture` cap URL is refreshed from
`SlCapabilities`); each texture is fetched on a background `IoTaskPool` task
(blocking HTTP off-thread, decode on the store's own rayon pool), and
`poll_textures` folds a finished decode into a shared cache and announces it
with a `TextureDecoded` message. `build_prim_faces` decodes the object's
`TextureEntry`, builds one `StandardMaterial` per face (tint now, texture
parked in `PrimTextures` until decoded), and `apply_prim_textures` uploads
(deduped) the diffuse `Image` into each parked material's
`base_color_texture`; a no-texture / failed face keeps its flat tint. The
P5.2 shared placeholder material is gone (each face owns its material).
**Terrain (P2.2) was migrated onto the same store**: `learn_composition` now
calls `manager.request`, and its detail textures arrive as `TextureDecoded`
(built with a tiling sampler) instead of the old
`Command::FetchTexture` / `TextureReceived` + inline `decode_j2c`, so the
viewer has one texture pipeline. New re-export: `CAP_GET_TEXTURE` from
`sl-client-bevy`. Verified live on OpenSim (prims render textured, incl. the
default plywood; terrain detail textures decode + tile; the on-disk cache
populates under `~/.cache/sl-client-bevy-viewer/texturecache`).
