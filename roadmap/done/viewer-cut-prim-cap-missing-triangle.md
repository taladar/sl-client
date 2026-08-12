---
id: viewer-cut-prim-cap-missing-triangle
title: Cut-prim path caps drop a triangle (filed as "prim LOD not reacting")
topic: viewer
status: done
origin: observed during shadow-cull profiling on aditi (2026-08-11)
refs: [viewer-profiling, viewer-p21-3]
---

Context: [context/viewer.md](../context/viewer.md).

Filed as "prim tessellation LOD does not react to camera distance", but the
investigation showed that was a **misdiagnosis**. The prim-LOD machinery works:
on aditi the per-tier target distribution tracks the camera continuously as it
moves and tens of thousands of `apply_prim_lod` re-tessellations fire
(`Low→Medium`, `Medium→Lowest`, …). The megaprim "walls" that looked stuck
report `lod=High` because they are huge (a ~28 m diagonal stays `High` inside
~112 m) — that is the correct tier, not a stuck one.

The **real** bug behind "the wall is degraded to triangles": a thin, thin
profile-**cut** box (picked live: path cut `[0.48, 0.50]`, profile cut
`[0.375, 0.875]`, scaled `20×20×0.5`). Its big flat faces are the path **caps**,
and a profile cut opens the ring, so the cap is built by `build_cap`'s fan
(`sl-prim/src/volume.rs`). The old fan added its **own** bounding-box centre
vertex and ran a `ring_count − 1` fan around it — which drops the closing
triangle (last profile point back to the first), tearing a triangular hole.

This is **not** megaprim-specific: any *solid, profile-cut* prim of any size (a
wedge, a pie-slice cylinder, cut trim) lost a cap triangle. Path-cut-only boxes
(closed ring), hollow prims (`build_hollow_cap`), and uncut boxes
(`build_uncut_cube_cap`) were unaffected — different cap paths.

**Fix:** `build_cap` now mirrors Firestorm's `LLVolumeFace::createCap`
(non-hollow branch) exactly — it fans `num_vertices − 2` triangles from the
**last** vertex. A closed ring still adds the centre vertex (its `gen_ngon`
seam repeats the first point, so `ring_count − 1` triangles close it); an open
ring takes **no** extra centre and fans `ring_count − 2` triangles from the
origin centre-pivot `gen_ngon` already pushed, filling the whole cross-section.
Regression test `cut_box_cap_fan_has_no_missing_triangle` (the picked wall's
exact params) asserts the cap's triangle area equals the full cross-section at
every LOD. Verified live on aditi: the wall panels render as solid rectangles.
