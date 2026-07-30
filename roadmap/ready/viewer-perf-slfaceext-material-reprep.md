---
id: viewer-perf-slfaceext-material-reprep
title: Investigate per-frame re-prep of SlFaceExt face materials during rez
topic: viewer
status: ready
origin: Tracy profiling of Aditi rezzing (2026-07-30)
refs: [viewer-perf-ui-layout-per-frame-relayout]
---

Context: [context/viewer.md](../context/viewer.md).

Tracy self-time over the first ~10 s of rezzing on Aditi shows the render-world
prepare of our custom face material (`SlFaceExt`, an `ExtendedMaterial` over
`StandardMaterial`) running **every frame**:

| System | ms/frame | n |
| --- | --- | --- |
| `erased_render_asset::prepare_erased_assets<…SlFaceExt>` | 1.07 | 203 |
| `par_for_each … Changed<MeshMaterial3d<…SlFaceExt>>` | 0.29 | 3444 |
| `material::check_entities_needing_specialization<…SlFaceExt>` | 0.11 | 203 |

Some of this is legitimate while prims stream in (new materials genuinely need
preparing). But the concern is whether existing `SlFaceExt` materials are being
**mutated every frame**, which marks them `Changed` and forces a full
`ExtendedMaterial` bind-group re-prepare — the exact trap recorded in the
`sl-client-facematerial-no-per-frame-mutation` memory (ExtendedMaterial re-prep
= full bind-group recreate; time-based face effects must be driven GPU-side from
`globals.time`, with params written once on change).

Investigate:

- Whether any system writes `Assets<…SlFaceExt>` / the material component each
  frame (texture-anim faces, hover/glow, alpha, bump) rather than only on an
  actual state change. The `Changed<MeshMaterial3d<…SlFaceExt>>` count (3444
  hits over 203 frames ≈ 17 changed entities/frame) suggests steady churn even
  once a face is stable.
- Whether the prepare cost tracks the number of *changed* materials (expected
  during rez) or a fixed per-frame set (a mutation bug).

Measure with a ≤10 s `tracy-grab.sh` capture while **stationary and fully
rezzed** — if the prepare stays ~1 ms/frame with nothing new rezzing, something
is mutating materials every frame.
