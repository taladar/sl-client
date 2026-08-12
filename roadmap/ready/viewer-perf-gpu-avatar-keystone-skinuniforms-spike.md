---
id: viewer-perf-gpu-avatar-keystone-skinuniforms-spike
title: GPU avatars keystone spike — validate the SkinUniforms write-in
topic: viewer
status: ready
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §2.4, §9.1(1)
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-gpu-avatar-phase1-gpu-fk-palettes]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §2.4 and §9.1
risk 1. Epic: [[viewer-perf-gpu-avatar-crowd]].

**The load-bearing assumption — de-risk before building Phase 1.** The whole
"don't fork Bevy's draw path" design depends on a compute pass being able to
write skin palettes **into `SkinUniforms.current_buffer`** (bound
`storage, read_write`) at Bevy-allocated offsets, with the write-ordering, the
per-frame current/prev buffer swap, and realloc-on-growth all holding.

Spike (toy branch, not for merge): bind `current_buffer` in a compute pass that
writes a constant/known palette for one skinned mesh; confirm on-grid:

- the mesh draws with the compute-written pose (not the CPU-staged one);
- motion vectors / TAA read last frame's posed palette (swap semantics);
- the swap-with-empty-staging edge (`prepare_skins` early-returns without
  swapping when staging is empty — our registered skins must keep it non-empty);
- no wgpu bind-usage validation rejection on desktop Vulkan.

If it fails: fallback is our own palette buffer pair + a forked `skinning.wgsl`
via a custom `MaterialExtension` vertex shader on avatar materials (more code,
same concept — must re-verify batching + bindless still hold). This spike also
answers whether the staging-bandwidth waste (rest junk re-uploaded for frozen
slots) matters. **Needs a live viewer run** (visual + motion-vector check);
a headless read-back render test covers the draw-output half.
