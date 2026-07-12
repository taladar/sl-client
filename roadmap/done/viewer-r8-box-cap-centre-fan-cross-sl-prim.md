---
id: viewer-r8
title: Box-cap centre-fan cross (sl-prim)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R8. Box-cap centre-fan cross (`sl-prim`).** Every plain box (cube)
showed an **X / cross** through each cap face's texture. `build_cap` built
the square cap as a centre-vertex **fan** (four triangles meeting at the
middle), and a real texture reveals the fan's diagonals as a cross. The
reference viewer never does this for a plain box — `createCap` routes a solid,
uncut, full-path square-on-line prim to `createUnCutCubeCap`, a proper
two-triangle quad grid (a `(grid_size + 1)^2` bilinear vertex grid, one quad
per cell). Ported as `build_uncut_cube_cap` / `uncut_cube_indices`
(+ `is_uncut_cube`) in `sl-prim` `volume.rs`, dispatched for that case; other
solid caps (round / cut / tapered) keep the fan (the reference viewer fans
those too, so they already match). Tests `box_caps_are_two_triangle_quads`
(Lowest LOD: 4 verts / 2 tris / corner UVs) and
`split_box_caps_are_a_consistent_grid` (High LOD: a square vertex grid, never
a fan). **User-confirmed: cube cross gone.**
