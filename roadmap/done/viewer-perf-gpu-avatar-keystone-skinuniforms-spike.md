---
id: viewer-perf-gpu-avatar-keystone-skinuniforms-spike
title: GPU avatars keystone spike — validate the SkinUniforms write-in
topic: viewer
status: done
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §2.4, §9.1(1)
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-gpu-avatar-phase1-gpu-fk-palettes]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §2.4 and §9.1
risk 1. Epic: [[viewer-perf-gpu-avatar-crowd]].

## Verdict (2026-08-12): CONFIRMED — write LANDED

The keystone **holds**. Headless render-readback tests pass, and a live Aditi
run on a **~106-joint Bento mesh-body** avatar logged
`palette[0] == expected (Marker mode, worst diff 0e0) — write LANDED` on
**89/89** samples with **no wgpu validation error, no panic, clean logout**. So
a compute pass binding `SkinUniforms.current_buffer` as `storage, read_write`
and writing palettes at Bevy-allocated offsets works bit-exact every frame on a
real mesh avatar — **Phase 1 builds on the write-in as designed; the
fork-`skinning.wgsl` fallback is not needed.**

The spike ships as a **flag-gated diagnostic** (default off): module
`sl-client-bevy-viewer/src/gpu_avatar_spike.rs` + `.wgsl`, enabled by
`SL_VIEWER_GPU_AVATAR_SPIKE=identity|marker`. It converges its target onto the
most-jointed skin (the mesh body) and logs a ~1 Hz `write LANDED` / `did NOT
land` readback verdict — a live verification tool for Phase 1. (Live-run gotcha
proven here: the target must be the *visible* mesh body — an early pick can
latch onto a hidden system base-body part; and a full palette overwrite
*displaces* a part rather than deforming it in place, so the readback verdict,
not the eyeball, is the signal.)

### Two findings that constrain Phase 1

- **`current_buffer` is `STORAGE | COPY_DST` (no `COPY_SRC`)** — any palette
  debug/validation readback must be a **compute copy** through the storage
  binding, not `copy_buffer_to_buffer`.
- **`current_skin_index` staleness** — Bevy bakes the skin offset into the mesh
  instance uniform only when the instance re-extracts; a *fully static* skinned
  mesh can be left at `u32::MAX` and render nothing. Real avatars re-extract
  every frame, but the Phase 4 frozen-skin endgame must guard against this.

Both are carried into [[viewer-perf-gpu-avatar-phase1-gpu-fk-palettes]].

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
