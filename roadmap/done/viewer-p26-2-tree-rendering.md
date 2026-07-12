---
id: viewer-p26-2
title: Tree rendering
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 26 — Linden trees & grass
---

Context: [context/viewer.md](../context/viewer.md).

**P26.2. Tree rendering.** Render pcode-tree objects as the reference
branching geometry, falling back to a distance billboard imposter
(`LLVOTree`), with the species diffuse texture through the texture pipeline.
Done: `sl-tree` grew a Bevy-free `geometry` module porting
`LLVOTree::updateGeometry` / `genBranchPipeline` — a recursive branch
pipeline stamping transformed copies of a tapered trunk **cylinder** (4
trunk LODs, the `sLODSlices` `{10,5,4,3}`) and a crossed-quad **leaf** card
into one `TreeMesh`, in Second Life Z-up at unit outer scale, plus a
`billboard_geometry` crossed-quad imposter. The trunk Perlin bark turbulence
(`LLPerlinNoise::turbulence3`) is ported in an `sl-tree::noise` module that
replicates glibc's TYPE_3 `random()` seeded with the C default `1` (what the
reference's `init()` implicitly draws from, having no `srand()`) and consumes
the stream in the same order (the `g1`/`g2`/`g3` draws then the permutation
shuffle), unit-tested against the canonical glibc seed-1 sequence — so the
bark matches a fresh-process reference. One faithful simplification remains:
wind/trunk-bend is not simulated (so droop is the rest value
`species.droop + 25°`). The winding, leaf-card layout, and the
quaternion→matrix conventions are ported verbatim (unit-tested against the
reference `LLQuaternion` vector-rotation formula). `sl-client-bevy` adds
`to_bevy_tree_mesh` and re-exports the geometry API; the viewer gains an
`ObjectCategory::Tree` (classified from `PCODE_TREE` / `PCODE_NEW_TREE`),
builds one face entity textured with the species diffuse (a synthetic white
`TextureFace` through the Phase-6 pipeline, `AlphaMode::Mask` so the
leaf-card / trunk alpha clips cutout foliage), and applies the reference
tree placement in a tree-specific geometry-holder transform (uniform
`scale.length() * 0.05` scale, 90° Z yaw, `-0.1 m` plant nudge). The
render-priority driver picks each tree's `TreeTier` from its on-screen
size — the branching LOD by distance, or the billboard imposter once tiny —
and `apply_tree_lod` regenerates on a change, the tree counterpart of the
prim LOD path. Verified live on OpenSim (a `rez_sample_trees` example rezzes
a stand of species): trunk bark + cutout leaf cards render correctly. Two
live findings baked in: OpenSim's vegetation module multiplies a rezzed
tree's scale by ~8 (`AdaptTree`), and the species texture is an atlas whose
transparent edges made a repeat-wrapped bilinear sample bleed through the
alpha mask at the trunk seam — fixed by a small `TRUNK_U_MARGIN` inset on
the seam column.
