---
id: viewer-perf-gpu-avatar-crowd
title: GPU avatar crowd — compute-pass animation + same-body instancing
topic: viewer
status: ideas
origin: design discussion off the 2026-08-12 critical-path capture
refs: [viewer-perf-steady-state-46fps-ceiling, viewer-perf-avatar-pose-extract-skins, viewer-avatar-impostors-billboard, viewer-perf-skeleton-single-solve, viewer-avatar-mesh-accurate-pick, viewer-perf-hover-pick-raycast]
---

Context: [context/viewer.md](../context/viewer.md). **Full design:**
[context/gpu-avatars.md](../context/gpu-avatars.md).

> **Epic — decomposed into phase tasks (2026-08-12).** The detailed,
> implementation-ready design lives in
> [context/gpu-avatars.md](../context/gpu-avatars.md); this file is the overview
> and rationale. Phases: [[viewer-perf-gpu-avatar-phase0-mesh-dedup]]
> (instancing unlock, in progress),
> [[viewer-perf-gpu-avatar-keystone-skinuniforms-spike]] (de-risk the
> `SkinUniforms` write-in first),
> [[viewer-perf-gpu-avatar-phase1-gpu-fk-palettes]] (GPU FK+palettes — kills the
> serial extract), [[viewer-perf-gpu-avatar-phase2-gpu-sample-blend]] (GPU
> sample+blend), [[viewer-perf-gpu-avatar-phase3-gpu-picking]] (GPU ID-buffer
> picking), [[viewer-perf-gpu-avatar-phase4-remove-scaffolding]],
> [[viewer-perf-gpu-avatar-phase5-lod-polish]]. Near-term no-GPU stopgaps:
> [[viewer-perf-animation-lod-pose-cache]].

The architecture bet for the **near animated crowd** — the dance-club case,
where impostors ([[viewer-avatar-impostors-billboard]]) and distance LOD do
**not** apply because everyone is close, visible, and dancing (so most joints
move every frame). This is the scenario the 2026-08-12 critical-path capture
([[viewer-perf-steady-state-46fps-ceiling]]) says we must design for, and it
is the one where the cheap levers fail.

## Why the cheap levers fail here, and why GPU is the fit

The frame is **co-limited** main-app ≈ render-app (~40 ms each) + a serial
`ExtractSchedule` (~7 ms). A busy club drives an inherent `O(avatars × joints)`
cost across **three** CPU stages that all scale together — CPU sample+blend
(`pose_avatar_skeletons`), Bevy **transform propagation** of `N × ~130` joint
entities in PostUpdate, and **`extract_skins`** (uploading the changed
matrices). Dancing means ~all joints change (per-joint `set_if_neq` ≈ useless)
and everyone is near (distance LOD / impostors ≈ useless).

GPU skeletal animation removes **all three** CPU stages at once: the
skinning-only joints leave the ECS transform graph entirely.

## The design (one coherent thing)

**Compute-pass GPU animation feeding same-body GPU-instanced draws.** The
sharing structure of a real crowd makes each piece cheap:

| Shared across avatars | Enables | Thread it helps |
| --- | --- | --- |
| **same mesh body** (popular bodies cover most residents) | one vertex/weight/inverse-bind buffer (already dedup'd) **+ GPU-instanced draws** — one indirect draw per body asset, per-instance skin palette + bindless/array textures | **render** (draws, queue/prepare, `camera_driver`) |
| **same animation asset** (often the same dance) | clip sampled **once per (asset, quantized phase)** in the compute pass | **main/extract** (sample dedup) |
| same body + anim + phase + shape | identical skin matrices → *fully* instanced incl. pose | both |

The key point: **same-body instancing hits the render thread; GPU animation
hits the main/extract thread — and the frame is co-limited between exactly
those two.** They are the pair that actually moves a co-limited crowd (cutting
only one floors at the other). See the ceiling bug's critical-path section.

Pipeline:

1. Upload each animation **clip** as a GPU asset once (joint × keyframe →
   rotation; static). Per avatar per frame upload only a compact **playback
   state** (active clip ids, playheads, priorities, ease weights) — bytes, not
   130 matrices.
2. **Compute pass:** sample active clips (dedup by (asset, phase-bucket)),
   apply SL's per-joint priority arbitration + ease blend, run FK against the
   avatar's own rest skeleton (shape/appearance/volume-bone offsets differ per
   avatar → per-instance), write the skin-matrix palette into a GPU buffer.
3. **Draw:** group by mesh asset; one instanced/indirect draw per body (and
   per head/hair/clothing asset — see caveats), each instance indexing its own
   skin-palette offset + its own material slot (bindless / texture array).

## Stays on the CPU (small, deliberate)

- **look-at, reach, locomotion IK** — world-state-dependent iterative solves
  touching few joints, run only when active: compute CPU, inject a **sparse
  additive correction** into the GPU blend.
- **Socket joints** (~5–20: hands, head, pelvis) that carry **attachments**,
  name-tag anchors, camera focus, foot references: CPU-FK just that short
  chain (cheap). Do **not** GPU-readback these — 1–2-frame lag on a
  fast-moving hand attachment would be visible.
- **Picking:** with every avatar GPU-posed, CPU raycast is fully out (it
  already only tests the bind pose — [[viewer-avatar-mesh-accurate-pick]]).
  The picking path becomes **GPU ID-buffer + async readback**: render a
  primitive/entity-id pass (alpha-tested), async-copy the pixel under the
  cursor (~1–2 frame latency, fine for hover/click), read depth at the same
  pixel and unproject for the world hit. This is pixel-perfect under
  pose/morph AND independently deletes the O(all-meshes)
  [[viewer-perf-hover-pick-raycast]] cost — so GPU pose and GPU picking are
  mutually enabling.

## Caveats (SL-specific)

- An avatar is body + head + hair + clothing layers + attachments, each a
  separately-rigged mesh — so instancing is **per mesh asset, not per avatar**.
  "40 Maitreya bodies with different skins/clothes" instances the *body* layer
  even though heads/hair/attachments vary. Per-avatar variation (textures,
  alpha/BOM masks, materials) rides a **per-instance material slot**.
- Morph/appearance (shape sliders, joint offsets) are **not** per-frame — set
  on appearance change — so they stay a one-time per-avatar rest-skeleton bake
  that the compute FK reads; no per-frame morph work.
- This is a **bespoke skinning path** (a real divergence from Bevy's
  `SkinnedMesh` + joint-entity system). It also sidesteps the joint-entity
  hierarchy bugs the memory notes track (pose-driver orphaning joint children;
  propagation stomping hand-written joint globals). Big project — justified by
  the club-crowd case specifically.

## Relationship to impostors

This is the **primary** crowd strategy; billboard impostors
([[viewer-avatar-impostors-billboard]]) are the reference viewer's
fidelity-sacrificing *workaround* for a slow full-geometry path. Making the
real path scale here keeps avatars full-quality on every tier and pushes the
"need impostors" threshold far out, so impostors are **deferred to an opt-in
low-end / extreme-count fallback** — not the near-term answer. We copy the
reference's goal (survive a crowd), not its mechanism.

## Sequencing

Land [[viewer-perf-animation-lod-pose-cache]] first (no-GPU stopgaps that work
even when everyone is near), then this as the structural answer. Relate to
[[viewer-perf-skeleton-single-solve]] (solve once per avatar, the CPU-side
precursor of "sample once per (asset, phase)").
