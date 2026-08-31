---
id: test-fake-grid-terrain-layerdata
title: The fake grid sends terrain — LayerData patches, wind, clouds
topic: test
status: done
origin: test-harness plan (2026-08-30)
points: 3
refs: [viewer-fake-grid-login-smoke]
blocked_by: [test-fake-grid-determinism]
---

Context: [context/testing.md](../context/testing.md).

Done (2026-08-31). `SimSession::send_layer_data(layer, patches, now)`
sends one `LayerData` message of any layer; `send_terrain(patches, now)`
sends a whole region's ground as at most `TERRAIN_PATCHES_PER_MESSAGE`
(four) patches per message in OpenSim's spiral order — the outer ring of
the patch grid from its south-west corner, then the next ring in
(`LLClientView.SendLayerTopRight` / `SendLayerBottomLeft`). It addresses
patches by grid position, so the wind layer's two same-position patches
go through `send_layer_data` instead. `SimSession::region_handle()` is
the new accessor the fake grid stamps its patches with.

`sl-fake-grid/src/terrain.rs` holds `Heightfield` (`Flat` / `Slope` /
`Ridge` / `Steps`, each a closed form over the region's metres) and
`TerrainFixture { heights, wind, clouds, composition }` with
`to_patches(handle)` (256 land patches), `wind_patches` (two — the east
then the north component, as OpenSim's `SendWindData` packs them),
`cloud_patches` (one), and `to_raw()` (the estate RAW32 download of the
same heights, at the finest height multiplier whose range covers the
field). `RegionConfig::terrain` and `::environment` carry them; the
handshake's terrain composition is no longer hard-coded in the region
identity, and a session whose scenario names no RAW heightmap now serves
its region's own ground, so the estate download matches what the viewer
stands on. The stock scenario registers a JPEG2000 solid for each of the
four default Linden detail-texture ids (`scenario::default_assets`, from
`sl-test-assets`).

Arrival-burst order is now own avatar → parcel overlay → parcel
properties → LAND (+ WIND / CLOUD) → fixture objects. The avatar has to
go first: a `LayerData` message carries no region handle, and the client
labels each patch with the handle it learned from that circuit's first
object update.

Acceptance met: `sl-proto/tests/sim_session.rs` pins the spiral order,
the message batching and the decoded heights over the real loopback (and
the wind/cloud one-message-per-layer shape); the tokio end-to-end suite's
`arrival_streams_the_regions_ground` sees all 256 `TerrainPatch` events
under the region handle with the fixture's heights; the Bevy smoke waits
for the same 256 patches through the real UDP path.
