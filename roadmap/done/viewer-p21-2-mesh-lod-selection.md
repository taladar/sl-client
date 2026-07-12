---
id: viewer-p21-2
title: Mesh LOD selection
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 21 — Distance / pixel-area LOD
---

Context: [context/viewer.md](../context/viewer.md).

**P21.2. Mesh LOD selection.** Port `LLVOVolume::calcLOD`: pick a
`MeshLod` from pixel area / distance × `RenderVolumeLODFactor`, request that
block, and swap on change via `MeshStore::set_lod`, rebuilding the Bevy
mesh. Reference: `LLVolumeLODGroup`. **Done:** a new `MeshLod::for_distance`
(`sl-proto`) ports `calcLOD` / `computeLODDetail` /
`LLVolumeLODGroup::getDetailFromTan` — `tan_angle = lod_factor * radius /
(distance * pi/3)` with the near-distance quadratic ramp, mapped through the
`{1, 2, 8} * 0.03` thresholds; `radius` is the full scale-vector length
(`getScale().length()`, **not** the half-diagonal used for pixel area — the
reference thresholds are tuned against it), and a new `DEFAULT_LOD_FACTOR`
(`RenderVolumeLODFactor`, `1.0`) is the quality knob. The `MeshManager` now
splits its requests like the P21.1 texture manager: an ordinary scene mesh is
fetched at a coarse `INITIAL_MANAGED_LOD` placeholder block and the
render-priority driver upgrades / downgrades it (`set_lod_for_area`) toward
the level its owning object's on-screen size warrants; a boosted worn
attachment stays at `MeshLod::FINEST`, unmanaged. The driver aggregates the
*finest* LOD any on-screen instance of a shared mesh needs (mirroring the
per-texture max pixel area), so a mesh reused by many objects is not thrashed
between levels by whichever instance is visited last. On a swap `set_lod`
fetches + decodes the new block, `poll_meshes` re-announces the mesh, and
`apply_object_meshes`
despawns the object's old submesh entities and rebuilds them from the new
geometry (fresh Bevy `Mesh` handles — so unlike the texture path there is no
in-place-refresh problem). Verified live on aditi: a mesh drops to a coarser
block as the camera recedes and rises again on approach. Verifying mesh LOD
also surfaced and fixed **two latent P21.1 texture-LOD bugs**: (a) a
full-resolution (discard 0) fetch used the `1/8`-rate byte *estimate*, which
under-fetches a resolution-progressive codestream — the partial decode
*succeeds*, so the decode-error fallback never fired and "full res" stuck at a
reduced size; now a discard-0 fetch uses the guaranteed-complete
`full_data_size_bound`, and the manager reads the true native size from the
J2C header rather than back-calculating it; (b) a texture that changed LOD
re-decoded but never *displayed* the new resolution, because `bevy_pbr` does
not rebuild a material's bind group when an `Image` it samples is replaced —
now the sampling materials are marked changed on re-upload. The crosshair pick
tool (`P`) gained a live LOD readout (a face's texture discard level + true
header-native size, and a mesh's decoded LOD) used to pin both bugs down; a
512² texture was confirmed cycling `discard 0 → 3 → 0` (512² → 64² → 512²) and
visibly re-sharpening on approach.
