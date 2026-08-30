---
id: test-shared-test-assets
title: sl-test-assets — one procedural asset source for every test tier
topic: test
status: ready
origin: test-harness plan (2026-08-30)
points: 2
refs: [viewer-render-scene-coverage]
---

Context: [context/testing.md](../context/testing.md).

The no-grid scene registry builds its textures and meshes as Bevy assets
in `SceneAssets`; the fake grid serves bytes by UUID from an in-memory
asset source. For a checker to be "red/green" in both, the *generator*
must be shared. Add a small no-Bevy crate `sl-test-assets`:

- `checker_rgba(size, a, b)`, `solid_rgba`, `gradient_rgba`;
- `j2c(&RgbaImage) -> Vec<u8>` through `sl-texture`'s `encode` feature;
- `unit_cube_mesh_asset() -> Vec<u8>` (an encoder if `sl-mesh` has one,
  otherwise a recorded blob under `fixtures/` with the generating code
  kept beside it);
- `mini_avatar_bakes()` keyed by the mini LAD's bake slots;
- four terrain detail solids under the default Linden detail UUIDs.

`SceneAssets` and `Scenario::assets` both consume it; a round-trip test
decodes the JPEG2000 back through `sl-texture` and checks the checker.
