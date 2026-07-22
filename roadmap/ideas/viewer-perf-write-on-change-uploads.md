---
id: viewer-perf-write-on-change-uploads
title: Write GPU-visible state only when it changed (morphs, sky, water)
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-perf-texture-anim-pause]
---

Context: [context/viewer.md](../context/viewer.md).

A cluster of systems that mutate GPU-visible assets/components every
frame with values that are identical most frames. Bevy re-extracts and
re-uploads anything marked changed, so every needless `get_mut` /
`deref_mut` is a needless upload. (The texture-animation instance of
this pattern is split out with extra behaviour work into
[[viewer-perf-texture-anim-pause]].)

## Instances

- **Avatar runtime morphs** — `apply_avatar_runtime_morphs`
  (`avatars.rs:3374-3390`): unconditional `&mut *weights` marks
  `MeshMorphWeights` changed **every frame for every avatar body part**,
  then overwrites each slot. For an idle avatar (no blink mid-cycle, no
  body-physics displacement) the values equal last frame's, yet every
  part re-uploads its morph weights. Fix: build the new weights in a
  scratch buffer and take `&mut` only when a slot actually differs.
  Note: actual skinning/morphing correctly runs on the GPU
  (`SkinnedMesh` / `MeshMorphWeights`) — this is only about the CPU-side
  change-marking.
- **Sky** — `drive_sky` (`sky.rs:547-553`) recomputes
  `blended_sky_settings` + `resolve_sky` (pure CPU trig over the whole
  frame settings) and writes `material.params` via `get_mut` every
  frame, though the day-cycle inputs move very slowly. Fix: cache
  `resolve_sky` keyed on a quantized day position / settings revision
  and skip the write when the resolved params are unchanged.
- **Water & cloud/star scroll** — `drive_water` (`water.rs`) and
  `drive_clouds`/`drive_stars` (`sky.rs:632`, `758`, `878`, `1050`) fold
  a time-based scroll/wave phase into their materials each frame. These
  are singleton materials (low absolute cost), but the phase can be
  computed **in-shader from Bevy's `globals.time`** so the CPU never
  touches the material at all; the remaining slow inputs (EEP settings,
  camera altitude) then gate the occasional real write, `set_if_neq`
  style.

## Estimated impact

Medium overall. The morph-weights item scales with avatar count × body
parts and is the main win (removes per-frame buffer re-extraction for
every idle avatar); the sky/water items are small per frame but are
always-on background cost on every scene, and moving scroll phases to
`globals.time` also simplifies the drivers. Combined with
[[viewer-perf-texture-anim-pause]] this establishes the codebase-wide
idiom: *compute first, compare, only then `get_mut`*. Verify via
[[viewer-profiling]] render-extraction spans (changed-asset counts per
frame should drop to near zero on an idle scene).

Confidence: high for the write patterns (all verified); medium for the
magnitude of the extraction/upload savings until profiled.
