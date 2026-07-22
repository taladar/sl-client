---
id: viewer-perf-flexi-settle-lod
title: Flexi prims — settle detection, distance LOD, stop per-frame re-upload
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

`simulate_flexi` (`flexi.rs:228-274`) runs **every frame for every flexi
prim** with no settle, distance, or visibility culling (the module
docstring itself notes "no screen-area LOD throttling"). Per prim per
frame it:

1. steps the flexible chain simulation;
2. calls `tessellate_with_path(&sim.shape, FLEXI_LOD, &path)` — a fresh,
   heap-allocated full prim tessellation at the fixed `PrimLod::High`
   (`flexi.rs:88`, `253`);
3. clones each face's position/normal `Vec`s into the mesh via
   `mesh.insert_attribute(..)` through `Assets::get_mut`
   (`flexi.rs:268-271`), which marks the mesh changed → a **full GPU
   vertex-buffer re-upload every frame**.

Flexi content (hair, skirts, jewelry chains, plants, flags) is numerous
in real scenes, and most of it is **near-static most of the time** — a
settled flexi chain barely moves until its parent moves or wind changes.

## Proposed fix (in payoff order)

1. **Settle detection:** track the chain's per-node positional delta /
   velocity; below an epsilon, skip the re-tessellation and the mesh
   write entirely (the chain state can keep integrating cheaply, or
   sleep until the parent transform / wind input changes). Most flexi
   prims then cost nothing per frame.
2. **Distance / pixel-area LOD:** lower the tessellation LOD for distant
   prims instead of the fixed `High` (mirror `apply_prim_lod`'s
   distance buckets, `objects.rs:2594`), and skip simulation for
   sub-pixel prims. The reference's flexi implementation does
   distance-based simulation throttling (`LLVolumeImplFlexible`).
3. **Allocation reuse:** tessellate into a persistent scratch buffer
   (and write attributes without the intermediate per-face `Vec`
   clones) so the frames that *do* update stop churning the allocator.

## Estimated impact

High on flexi-dense scenes: each settled/culled prim removes one full
prim tessellation + one vertex-buffer upload per frame. With, say, 40
flexi prims in view and 90% settled at any instant, that removes ~36
tessellations and ~36 GPU uploads per frame. Even the always-moving
minority get cheaper via LOD. Measure with [[viewer-profiling]]
(`simulate_flexi` zone self-time + allocation counts in Tracy memory
mode) on a flexi-heavy test scene.

Confidence: high — code path, fixed LOD, and per-frame `get_mut`
verified; no existing gating found.
