---
id: viewer-perf-gpu-particles
title: GPU-instanced particle rendering
topic: viewer
status: done
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-perf-frame-churn-cleanups]
---

Context: [context/viewer.md](../context/viewer.md).

**Done — Option 1 (instance buffer, CPU sim).** The Phase 30 particle render
was fully CPU-side: every frame, per source, it rebuilt a five-attribute
billboard mesh (`build_cloud_mesh`) and `meshes.insert`ed it — a full
vertex-buffer re-upload per source per frame, plus the camera-facing quad math
on the CPU. That is now GPU-instanced:

- **One shared unit-quad mesh** (`ParticleQuad`), uploaded once at startup and
  never rebuilt — every cloud instances it.
- The CPU simulation (`Emitter` / `Particle` / `integrate`) is unchanged; per
  frame each cloud produces a compact per-particle `ParticleInstance` buffer (52
  bytes/particle: world position, size, colour, velocity, flags) in a component
  instead of a mesh. That instance buffer is the *only* per-frame upload, reused
  in place (`RawBufferVec`, reallocated only when a cloud grows).
- A **custom instanced pipeline** (`src/particle_render.rs`, `particle.wgsl`),
  derived from Bevy's `MeshPipeline`, expands each particle into a camera-facing
  billboard in the vertex shader (the `LLVOPartGroup::getGeometry` port, incl.
  `FOLLOW_VELOCITY`) and shades it in the fragment: textured × per-instance
  tint, then **faithful PBR lighting** for non-emissive clouds or unlit
  passthrough for emissive / additive / HUD clouds (the reference only forces
  `FULLBRIGHT` on `EMISSIVE` particles — `llvopartgroup.cpp:359`). Pipeline
  specialized by blend mode (additive vs alpha) and lit/unlit.
- HUD vs world scoping (P35.4) is preserved via each view's
  `RenderVisibleEntities` (render-layer filtered by `check_visibility`); a
  `NoFrustumCulling` cloud stays in that list.
- The custom shader carries absolute world positions and never reads the mesh
  transform, so a plain `draw_indexed` is correct and the main view keeps Bevy's
  GPU indirect drawing / culling (no scene-wide `NoIndirectDrawing`).

Testability preserved: the headless `render_test` harness reads the per-particle
positions from the instance buffer (a "point cloud"), so the NaN-over-time /
change / non-vacuous-render checks still bite. New unit tests cover the instance
record, the quad, the instance-buffer layout, and the blend derivation.

## Correctness fixes found in live testing (aditi fountain)

Three issues surfaced on real particle content and were fixed here:

- **Whole-stream flicker on camera motion.** Every cloud instances the same
  shared quad through the same pipeline, so Bevy's GPU-preprocessing batcher
  merged sort-adjacent clouds into one draw — and our per-item instance-buffer
  draw then rendered only the first, so a camera-dependent merge dropped whole
  streams. Fixed by tagging each cloud `NoAutomaticBatching` (its own draw,
  indirect drawing kept for the rest of the scene).
- **Particles drawn into reflection-probe captures.** `queue_particles` now
  skips the probe capture cameras (`order < 0`), so particles are not frozen
  into a probe's image-based lighting (the reference likewise excludes them).

**Still open:** particles order wrong against the **translucent water** surface
(the water paints over particles in front of it). A water depth-write fix was
tried and reverted — it hides the translucent content *behind* the water. The
real fix is the reference's pre/post-water alpha bucketing across the whole
transparent pass; filed as [[viewer-particle-water-ordering]].

## Deferred follow-ups

Split out into [[viewer-perf-gpu-particle-sim]]: Option 2 (GPU **compute**
simulation, only worth it if profiling ever shows integration — not the upload —
dominating; and the tie-in for **raising the 4096 particle cap**), plus the
still-unported reference **emission-rate LOD throttling** for distant /
off-screen sources.

The independent quick fix — the per-frame `ops.system.clone()` of the whole
`ParticleSystem` per source — remains filed in
[[viewer-perf-frame-churn-cleanups]].

## Original analysis (retained)

Options, in ascending ambition:

1. **Instance buffer, CPU sim** — keep the existing simulation, replace
   `build_cloud_mesh` + `meshes.insert` with a per-source instance buffer
   (custom material/pipeline). Most of the win, least risk. **← implemented.**
2. **GPU simulation (compute)** for the simple kinematic patterns (ballistic +
   wind + drag), falling back to CPU for pattern types that need scene queries
   (target-omega, follow-source). Only worth it if profiling shows integration
   itself (not the upload) dominating. **← deferred, see above.**
