---
id: viewer-f3-overlay-additional-asset-stores
title: Extend the F3 pipeline overlay to the asset stores added since it was built
topic: viewer
status: done
origin: noticed while adding the failure-edge deferred count to F3 (2026-08-11)
---

Done (2026-08-11): each of the five later-added managers (`AnimationManager`,
`EnvironmentAssetManager`, `SoundCache`, `WearableAssetManager`,
`MaterialManager`) gained `stats()` / `gate_stats()` / `deferred_count()`
delegating to its wrapped `AssetStore` (deferred = the cap-not-set `pending`
queue; `MaterialManager` also counts slot patches parked on undecoded textures).
The overlay shows the texture and mesh stores as full two-line blocks and the
five new stores as one condensed `format_store_line` each (`anim`, `env`,
`sound`, `wear`, `gmat` — `gmat` is the glTF-material *asset* store, distinct
from the interned `FaceMaterial` `mat` cache line), keeping the panel at 12
lines. Audit confirmed no other `AssetStore`-wrapping managers exist (notecards
/ LSL scripts fetch ad hoc; `LegacyMaterialManager` / `BumpManager` POST
`RenderMaterials` directly and hold no store). Tests updated in
`diagnostics::tests`.

Context: [context/viewer.md](../context/viewer.md).

The F3 pipeline overlay (`diagnostics.rs`, `format_pipeline`) was built for the
first two asset pipelines and still shows only:

- the **texture** store (`TextureManager`),
- the **mesh** store (`MeshManager`),
- the **geometry** cache (cross-instance tessellation reuse),
- the **material** cache (interned `FaceMaterial`s).

Several asset managers with their own `AssetStore` (fetch / decode / disk cache
/ priority gate) were added later and are **not** shown, so "nothing left to
load" on F3 ignores them entirely:

- **animation** assets (`animations.rs`, `AnimationManager`),
- **settings / environment** assets (`environment_assets.rs`,
  `EnvironmentAssetManager`),
- **sound** clips (`sound_cache.rs`, `SoundCache`),
- **wearable / bake-input** assets (`bake_inputs.rs`, `WearableAssetManager`),
- **glTF material** assets (`materials.rs`, `MaterialManager`) — distinct from
  the `FaceMaterial` *cache* already shown as `mat`.

Audit for any others (notecards, gestures, LSL script assets, etc.) while here.

## Task

Give each manager the same `stats()` / `gate_stats()` accessors the texture and
mesh managers expose (most wrap an `AssetStore` that already has `stats()` /
`gate_stats()`), plus the `deferred_count()` added in the 2026-08-11
failure-edge work ([[viewer-asset-failure-edge-retry]]) so parked / retrying
fetches are visible, and add one `format_store_block` line per store to
`format_pipeline`. Keep the panel compact (it is a debug overlay) — consider a
one-line-per-store condensed form if five more two-line blocks make it too tall.
Update the `diagnostics::tests` panel assertions.
