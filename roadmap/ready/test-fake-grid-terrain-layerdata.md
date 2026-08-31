---
id: test-fake-grid-terrain-layerdata
title: The fake grid sends terrain — LayerData patches, wind, clouds
topic: test
status: ready
origin: test-harness plan (2026-08-30)
points: 3
refs: [viewer-fake-grid-login-smoke]
blocked_by: [test-fake-grid-determinism]
---

Context: [context/testing.md](../context/testing.md).

A viewer logged into the fake grid sees no ground: `encode_layer` exists
in `sl-proto` but nothing sends it. Add
`SimSession::send_layer_data(layer, patches, now)` and
`send_terrain(patches, now)` (at most four patches per message, in
OpenSim's spiral order), with loopback tests; a `TerrainFixture {
heights: Heightfield (flat / slope / ridge / steps), wind, clouds,
composition, detail_textures }` with `to_patches(handle)` and `to_raw()`
so the estate RAW download matches the rendered ground;
`RegionConfig::terrain` and `::environment` (the terrain composition
today is hard-coded in the region identity). Register the four default
detail textures as JPEG2000 solids so the ground is not a failed fetch.

Arrival-burst order becomes: own avatar `ObjectUpdate` first — the client
learns a circuit's region handle from its first object update, and
patches before it are stamped with handle zero — then parcel overlay and
properties, then LAND (+ WIND/CLOUD), then the fixture objects.

Acceptance: the tokio end-to-end suite sees 256 `TerrainPatch` events
with the region handle; the Bevy smoke sees the terrain patch entities.
