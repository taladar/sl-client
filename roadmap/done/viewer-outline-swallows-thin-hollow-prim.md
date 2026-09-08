---
id: viewer-outline-swallows-thin-hollow-prim
title: The selection outline fills a thin-walled hollow prim white instead of outlining it
topic: viewer
status: done
origin: seen while diagnosing the animesh box shell on aditi (2026-09-08)
refs: [viewer-mesh-objects-outlined-by-a-shell, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Selecting a 95%-hollow box (0.5 × 0.5 × 1.49 m, so ~1.25 cm walls — the prim
from [[viewer-animesh-transparent-box-shell]]) makes it **light up completely
white** rather than showing a silhouette.

## Why

A prim's selection highlight was an **inverted hull**: the face mesh re-drawn
at `OUTLINE_INFLATE = 1.035` with front faces culled, so the sliver that
escapes the real surface reads as an edge
(`sl_viewer_edit::edit_selection::spawn_outline_overlay`).

That scale is a fraction of the **whole prim**, not of the wall it is
outlining. On a solid box the 3.5% sliver is thin against a 0.5 m face and
reads as an edge. On a hollow box the drawn surfaces are 1.25 cm thick, and
3.5% of 0.5 m is 1.75 cm — *wider than the wall itself*. The inflated hull
therefore clears the geometry entirely along the whole face instead of only
at its rim, and the outline covers the prim rather than bounding it. (What is
actually drawn is the *inner* wall: its normals point into the hollow, so it
is the back-facing half the cull keeps, and the inflate carries it out past
the outer wall.)

The same argument applies to any prim whose visible surface is thin relative
to its bounding box: a hollow cylinder, a heavily cut torus, a flattened box.

## FIX (2026-09-08): the reference's silhouette edge walk, ported

The faithful answer, not the fall-back the first draft of this task
described: the reference does not inflate anything, so nothing about its
outline scales with the bounding box. New `sl_viewer_edit::selection_silhouette`
ports both halves of it:

- `LLVolume::generateSilhouetteVertices` — each triangle classified toward /
  away by the sign of `(camera - v0) · n` on its **geometric** normal, and an
  edge kept when it has no neighbour in the face (the grid's boundary) or its
  neighbour is classified the other way. Neighbours are derived by matching
  positions rather than read out of a profile/path grid, because this
  viewer's faces arrive as plain indexed triangle lists;
- `LLSelectNode::renderOneSilhouette` — each such edge drawn as a **quad**:
  the edge, plus the edge pushed out along its two vertex normals by
  `silhouette_thickness` (`view_distance × 0.01 × fov / 60°`, so a constant
  width on screen), the outer side faded to zero alpha and the colour doubled
  at the surface.

That cannot fill a prim at any width: every vertex the ribbon emits is a
source vertex moved along **its own** normal, so no surface is ever displaced
across another one. The unit test `a_ribbon_only_ever_moves_along_the_normal`
pins exactly that, with a ribbon wider than the whole fixture cube.

Because the edge set is view-dependent, `update_selection_silhouettes`
re-derives it as the view moves — on a turn of more than
`SILHOUETTE_REBUILD_ANGLE` or a width change past `SILHOUETTE_REBUILD_WIDTH`,
stalest first, under the reference's own per-frame budget
(`MAX_SILHOUETTES_PER_FRAME` = its `MAX_SILS_PER_FRAME` = 50). The reference
regenerates only when the *object* moves, which leaves its own outline stale
while you orbit a stationary prim.

Not ported: the scrolling highlight texture (`sHighlightUAnim`), the
five-pixel line width (`wgpu` has no line-width state — the ribbon is
geometry, so this one does not bite here), the HUD-attachment zoom
substitution (a HUD face cannot be picked: the world pick excludes that
render layer), and `RenderHiddenSelections`, the reference's second pass that
draws the outline *through* intervening geometry while the build floater is
open — see [[viewer-selection-hidden-silhouette]].

A mesh object and a rigged face are unaffected: they take the wireframe path
([[viewer-mesh-objects-outlined-by-a-shell]]), which hugs the surface.

## Verified on the local grid (2026-09-08)

A prim wears a glowing edge outline instead of a filled surface, and the
95%-hollow box it was reported on is outlined rather than swallowed.

The run also turned up [[viewer-underwater-fog-swallows-translucency]], which
is not this and not new: submerged, with the **void** behind the object, the
outline is not drawn at all, because the submerged fog pass runs after the
transparent phase and fogs by the depth of whatever is *behind* a
non-depth-writing draw — 4 km of water where that is the void. It swallows
every alpha-blended surface underwater, not only this one.
