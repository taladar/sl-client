---
id: viewer-perf-flexi-distance-lod
title: Flexi prims — distance / pixel-area LOD and tessellation-allocation reuse
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-perf-flexi-settle-detection]
---

Context: [context/viewer.md](../context/viewer.md).

The two remaining bullets of the original flexi perf-survey item, after
[[viewer-perf-flexi-settle-detection]] landed the settle latch (which already
removes the per-frame tessellation + GPU upload + chain step for the near-static
majority of flexi prims).

## 1. Distance / pixel-area LOD

`simulate_flexi` tessellates the flexi profile at a **fixed** `FLEXI_LOD =
PrimLod::High` (`flexi.rs`), regardless of the prim's on-screen size. The
reference's flexi implementation (`LLVolumeImplFlexible`) does distance-based
simulation throttling and profile-resolution LOD. Two sub-parts:

- **Lower the tessellation LOD for distant prims** (mirror `apply_prim_lod`'s
  distance buckets, `objects.rs`) instead of the fixed `High`, and skip
  simulation entirely for sub-pixel prims.
- **Architectural note (why this is not trivial):** flexi face meshes are
  rewritten *in place* every moving frame, so the profile point count must stay
  constant between the initial build and the deform — that is exactly why the
  LOD is fixed today (`flexi.rs`, `FLEXI_LOD` doc). Per-distance LOD therefore
  means **respawning the face entities** when a prim crosses a distance bucket
  (like `apply_prim_lod` does with `despawn_prim_faces` / `spawn_prim_faces`,
  re-seeding `FlexiSimState::face_entities`), plus feeding the camera distance
  into the system. It needs live visual verification of LOD transitions (no
  popping / cross-section shear at bucket edges).

Note the settle latch caps the payoff: only the *moving* minority of flexi prims
re-tessellate at all, so distance LOD helps distant **moving** flexi (a windy
field of flexi grass, a distant dancer's hair) rather than the whole scene.

## 2. Tessellation-allocation reuse

`tessellate_with_path` returns a fresh `Prim` (per-face `Vec` allocations) each
moving frame, and `simulate_flexi` then clones each face's positions / normals
into the mesh. Tessellate into a **persistent scratch buffer** on the
`FlexiSimState` (and write attributes without the intermediate per-face `Vec`
clones) so the frames that *do* update stop churning the allocator. Lower value
now that settled prims skip tessellation altogether — this only helps the moving
minority — so it is bundled here rather than pursued on its own.

## Measurement

[[viewer-profiling]] — `simulate_flexi` zone self-time and allocation counts
(Tracy memory mode) on a flexi-heavy scene with a **moving** flexi majority
(wind / motion), where the settle latch does not already zero the cost.

## Parity-audit addendum (2026-08-19)

The reference also scales flexi *update rate*, not just tessellation:
`RenderFlexTimeFactor` is a sim-time / update-rate knob that lets distant
or numerous flexi prims simulate at a reduced rate. Our flexi sim runs
per-frame with settle detection (`sl-client-bevy-viewer/src/flexi.rs`)
and has no rate scaling; add the update-rate factor to this task's scope
alongside the distance/pixel-area LOD.
