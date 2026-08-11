---
id: viewer-f3-overlay-additional-asset-stores
title: Extend the F3 pipeline overlay to the asset stores added since it was built
topic: viewer
status: ready
origin: noticed while adding the failure-edge deferred count to F3 (2026-08-11)
---

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
