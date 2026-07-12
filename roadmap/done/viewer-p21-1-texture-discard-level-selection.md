---
id: viewer-p21-1
title: Texture discard-level selection
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 21 — Distance / pixel-area LOD
---

Context: [context/viewer.md](../context/viewer.md).

With per-object pixel area available (P20.1), fetch only the fidelity the view
warrants: coarser textures and meshes for small / distant objects, upgrading
as the camera approaches. The stores already expose `set_lod` for
upgrade/downgrade and the LOD newtypes have `finer()` / `coarser()`.

**P21.1. Texture discard-level selection.** From the P20.1 pixel area
choose a `DiscardLevel` (fewer pixels → coarser); request at that level and
upgrade / downgrade via `TextureStore::set_lod` as the camera approaches /
recedes, respecting the read-lease. Reference:
`LLViewerTexture::updateVirtualSize`. **Done:** a new
`DiscardLevel::for_pixel_area` (`sl-proto`) ports the reference viewer's
`discard = floor(log4(full_texels / on-screen area))`
(`LLViewerLODTexture::processTextureStats`) — computed by repeated division by
four rather than a float `log`, so a small / distant face selects a coarser
level, using the texture's *native* (discard-0) dimensions so the same
on-screen area maps to different levels for a 512² vs a 2048² texture. The
`TextureManager` now splits its requests: an ordinary prim / mesh / sculpt
diffuse face is **pixel-area LOD managed** (`request_face`) — first requested
at a coarse placeholder level (`INITIAL_MANAGED_DISCARD`, ¼ linear) that loads
fast, then upgraded / downgraded by the render-priority driver
(`set_lod_for_area`, called alongside `set_priority` each throttled frame) via
`TextureStore::set_lod` once the first decode reveals the native size. The
store's `set_lod` fetches + decodes on an upgrade (growing the same codestream
prefix — no re-fetch of bytes already in hand) and downsamples in place on a
downgrade, waiting on the entry's GPU read-lease; the completed image is
folded back in by `poll_textures` and re-uploaded *behind its existing Bevy
image handle* (`build_prim_image` / `Assets::insert`), so every material
sampling the texture shows the new resolution with no material re-patching.
The initial
request handle is retained for a managed texture (rather than dropped on
resolve as in P20.2) so its store entry stays live for later `set_lod`.
**Boosted textures stay full-resolution from the first fetch and are never LOD
managed** — an avatar body part / bake, a worn attachment, a HUD attachment
(all carry `AVATAR_BOOST` via `worn_base_priority`, which covers HUD
attachment points), and terrain detail textures (`TERRAIN_BOOST`): a boosted
request even *promotes* a texture a prim face had been managing
(`upgrade_to_full`), so a shared id (e.g. a terrain texture reused on a prim)
is never left coarse.
Verified live: OpenSim (avatar + terrain render sharp, no regression) and a
dense aditi region (441 LOD decisions — 280 downgrades, 161 upgrades — 0
failures, 507 textures drained through the gate, the own avatar full-res and
crisp at 60 FPS on a 35k-entity region).
