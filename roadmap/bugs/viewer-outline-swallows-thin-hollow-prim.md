---
id: viewer-outline-swallows-thin-hollow-prim
title: The selection outline fills a thin-walled hollow prim white instead of outlining it
topic: viewer
status: bugs
origin: seen while diagnosing the animesh box shell on aditi (2026-09-08)
refs: [viewer-mesh-objects-outlined-by-a-shell, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Selecting a 95%-hollow box (0.5 × 0.5 × 1.49 m, so ~1.25 cm walls — the prim
from [[viewer-animesh-transparent-box-shell]]) makes it **light up completely
white** rather than showing a silhouette.

## Why

A prim's selection highlight is an **inverted hull**: the face mesh re-drawn
at `OUTLINE_INFLATE = 1.035` with front faces culled, so the sliver that
escapes the real surface reads as an edge
(`sl_viewer_edit::edit_selection::spawn_outline_overlay`).

That scale is a fraction of the **whole prim**, not of the wall it is
outlining. On a solid box the 3.5% sliver is thin against a 0.5 m face and
reads as an edge. On a hollow box the drawn surfaces are 1.25 cm thick, and
3.5% of 0.5 m is 1.75 cm — *wider than the wall itself*. The inflated hull
therefore clears the geometry entirely along the whole face instead of only
at its rim, and the outline covers the prim rather than bounding it.

The same argument applies to any prim whose visible surface is thin relative
to its bounding box: a hollow cylinder, a heavily cut torus, a flattened box.

## Shape of a fix

The reference does not have this problem because it does not inflate
anything: `LLSelectMgr::renderOneSilhouette` walks the volume's actual
silhouette edges and draws *those*, so the outline is a property of the edge
set rather than of the bounding box. Porting the edge walk is the faithful
answer.

Short of that, the inflation has to stop being a fraction of the prim.
[[viewer-mesh-objects-outlined-by-a-shell]] already had to make the *mesh*
wireframe's lift a world distance for the same class of reason — the clamp
has to be in metres, not in units of the object. The hull needs the same
treatment: a small world-space offset along the normal, clamped so it never
exceeds the local wall thickness.

Note this only affects the **shell** path. A mesh object and a rigged face
take the wireframe path instead, which hugs the surface and does not inflate.
