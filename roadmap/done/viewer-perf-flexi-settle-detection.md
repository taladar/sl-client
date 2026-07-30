---
id: viewer-perf-flexi-settle-detection
title: Flexi prims — settle detection (stop per-frame re-tessellation / re-upload)
topic: viewer
status: done
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

`simulate_flexi` (`flexi.rs`) ran **every frame for every flexi prim** with no
settle culling. Per prim per frame it stepped the chain, called
`tessellate_with_path(&sim.shape, FLEXI_LOD, &path)` — a fresh, heap-allocated
full prim tessellation at the fixed `PrimLod::High` — and cloned each face's
position / normal `Vec`s into the mesh via `Assets::get_mut`, which marks the
mesh changed → a **full GPU vertex-buffer re-upload every frame**.

Flexi content (hair, skirts, jewelry chains, plants, flags) is numerous in real
scenes, and most of it is **near-static most of the time** — a settled flexi
chain barely moves until its parent moves or its parameters change.

This task is the **settle-detection** part of the original combined perf survey
item. The other two bullets (distance / pixel-area LOD, tessellation-allocation
reuse) are split out to [[viewer-perf-flexi-distance-lod]].

## Resolution

A **settle latch with input-change wake**, landed across `sl-prim` and the
viewer.

The first design considered — accumulate per-frame movement and re-upload once
the drift crosses a threshold — was rejected after measuring the solver: a
settled chain does **not** reach a true fixed point but a tiny residual limit
cycle (~0.1 mm/step). Summing per-step movement magnitudes over-counts that
in-place jitter, so a "settled" prim would still re-upload periodically, and the
signal cannot tell settled jitter from slow *coherent* motion (which must stay
smooth). So the movement is used as a **latch trigger**, not an accumulator.

1. **Movement signal (`sl-prim::flexi`).** `FlexiChain::step` and
   `integrate` now return how far the chain moved that frame — the
   largest single-step node displacement in metres, the pinned **anchor**
   included so an anchor-only glide (the prim moves without bending) still reads
   as motion. Pure and unit-tested (`step_movement_decays_as_the_chain_settles`,
   `moving_the_anchor_reports_movement`). The fixed-timestep accumulator returns
   0.0 on a frame that banks time without draining a whole step, which is
   correct (the geometry did not change that frame).

2. **Settle latch (viewer `flexi.rs`).** `FlexiSimState` gets a new
   `rest: Option<FlexiRest>`. While the chain is moving the prim re-tessellates
   and re-uploads every frame (so genuine motion stays smooth). Once a step
   moves less than `STEP_SETTLE_EPSILON` (0.3 mm — above the residual limit
   cycle) it does one final rewrite and **latches**, recording the pose /
   attributes / scale it settled at. A latched prim whose inputs still match is
   skipped **entirely** — no chain step, no tessellation, no GPU upload — so the
   near-static majority of flexi content costs nothing per frame. It **wakes**
   on any input change: the anchor gliding ≥ 1 mm from the recorded rest
   position (compared against the *rest* pose, not the previous frame, so a
   sub-threshold drift still accumulates), a spin past a small quaternion
   tolerance,
   or a scripted gravity / user-force / tension / size change. Because a skipped
   prim's geometry is not changing, the `Aabb` it keeps from its last rewrite
   stays correct, so frustum culling and ray-cast picking
   ([[viewer-flexi-prim-picking]]) still track it. Two ECS tests cover the latch
   (`a_settled_flexi_freezes_and_stops_rewriting`,
   `moving_a_settled_flexi_wakes_it`).

Skipped frames feed the chain no time, so a long stint frozen never turns into a
catch-up spike on wake (unlike a naive "keep integrating" approach) — the
fixed-timestep accumulator only banks the `dt` it is actually handed.

## Estimated impact

High on flexi-dense scenes: each settled prim removes one full prim
tessellation, one vertex-buffer upload, and the chain step, per frame. With ~40
flexi prims in view and 90% settled at any instant, that removes ~36
tessellations, ~36 GPU uploads, and ~36 chain steps per frame. The always-moving
minority pay the full cost (correctly). Confirm with [[viewer-profiling]]
(`simulate_flexi` zone self-time collapsing on a settled flexi-heavy scene).
