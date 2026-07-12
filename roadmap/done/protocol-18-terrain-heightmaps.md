---
id: protocol-18
title: Terrain heightmaps
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**18. Terrain heightmaps — `LayerData` (LAND/WATER/WIND/CLOUD) · 8 pts. ✅
Done.** Decodes the patched-DCT-compressed terrain layers into per-region
heightmaps — the ground for a renderer. New `sl-proto/src/terrain.rs` is a
faithful port of the viewer's decoder (`indra/llmessage/patch_code.cpp` +
`patch_idct.cpp`, which agree with OpenSim's `TerrainCompressor.cs`): an
MSB-first `BitReader` (matching LL's `LLBitPack::bitUnpack`, little-endian byte
reassembly), the group/patch headers, the run-length/sign/magnitude entropy
decode (with the `10` end-of-block and `97` end-of-patches markers), and the
2-D inverse DCT (dequantize + un-zigzag via the de-copy matrix, an inverse-DCT
column pass then a row pass with the `2/size` normalisation), scaled to heights
via `range/2^prequant` and the `dc_offset`. Handles both standard 16×16 patches
and the variable-region 32×32 "extended" (`'M'`/`'X'`/`'9'`/`':'`) layers (10-
vs 32-bit patch ids). New value types `TerrainLayerType` (the four layers, their
extended variants, and `Unknown`) and `TerrainPatch` (region handle, layer, grid
position, size, row-major `values`, with a `value(x, y)` accessor). Decoded in
`try_dispatch_object` so it runs on the **root and every child circuit**
(neighbour terrain streams too); cached per sim then `(layer, x, y)` and dropped
with the sim's other state on `DisableSimulator`/handover/relogin. Because
`LayerData` carries no region handle, the session learns each sim's handle from
its object updates and `EnableSimulator` (a `regions` map) and labels the
patches with it. Surfaced as `Event::TerrainPatch`; public API
`Session::terrain_patches()` / `terrain_patches_in_region(handle)` /
`terrain_height(x, y)` (root-region LAND). Wired through both runtimes
(re-exports + the exhaustive example/survey event matches; no command — terrain
is sim-pushed). Covered by four `sl-proto` unit tests (bit-reader round-trip, a
flat-patch closed-form height, the zero-size reject, and the end-of-patches
case) plus a `lifecycle.rs` end-to-end test (a synthesised `LayerData` datagram
→ `Event::TerrainPatch` + `terrain_height`). *Live-verified against the local
OpenSim via the new `terrain_probe` tokio example: a single login decoded all
**256 LAND patches** (the full 16×16 grid of a 256×256 region) with a sensible
ground-height range (≈ −0.1..25 m), plus the wind/cloud/water layers. Test:
local OpenSim.*
