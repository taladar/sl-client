---
id: viewer-render-closedness-check
title: Closedness check — an object's faces must enclose a volume
topic: viewer
status: ready
origin: the viewer-render-test-harness work (2026-07); written, found too noisy to trust, and removed rather than shipped
blocked_by: [viewer-render-test-harness]
refs: [viewer-render-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

A universal check for [[viewer-render-test-harness]]: an object's faces together
enclose a volume — every edge is shared by **exactly two** triangles. An edge
used by one is a **hole** (a dropped end cap, a profile cut whose edge faces
were never built, a sculpt seam that failed to stitch); an edge used by three or
more is a fold. Both render as *something*, plausibly, from most angles, which
is why neither is noticed until a user walks around the back.

This was **written and then removed**, and the removal is the point of the task:
it produced findings on correct geometry, and a noisy check gets ignored and
then deleted. Shipping one that cannot be trusted on day one would have cost the
whole suite's credibility. What follows is what it cost to learn, so the next
attempt starts from it rather than rediscovering it.

## What was already got wrong, and fixed

- **It must group per object, not per face.** A prim is an object entity with
  one child per face, because that is how the viewer builds one. A single face
  of a box is a flat quad and is *supposed* to be open, so the per-face check
  reported every correct prim as broken. `Geometry::group` exists for this and
  stayed behind — the union over the group is the level at which "solid" means
  anything.
- **It must match by quantized position, not by index.** A flat-shaded mesh
  duplicates its corners so each face can carry its own normal, so its shared
  edges have *different indices at the same point*. An index-based check reports
  a correctly closed cube as twelve holes.

## What defeated it, and is the actual work

Second Life tessellation emits **coincident vertices** — measured, on a twisted
torus: at every LOD, the closest distinct pair of vertices in a face is
`0.000000 m` apart. They are seam duplicates and they are correct. But a
position-quantized edge map cannot then distinguish:

- a seam duplicate (two coincident vertices, one real edge, correct), from
- a genuine fold (two triangles wrongly sharing an edge with a third).

The removed version reported the correct torus as "288 of its 6336 edges are
folded". Degenerate pole fans (a sphere's poles, where a whole ring collapses to
one point) are the same problem in a different shape.

So the check needs a way to tell duplication from folding. Candidates,
unexplored:

- Weld coincident vertices **first**, into a single index space, and count edges
  in that — which is what "shared" should have meant all along, and reduces the
  question to whether the weld is correct.
- Count **boundary** edges only (used exactly once) and ignore over-shared ones,
  which drops fold detection but may be reliable enough to be worth having
  alone.
- Take the winding into account: a correctly closed surface has each edge
  traversed once in each direction. That distinguishes a fold from a duplicate
  directly, and is the standard answer — it needs the weld above to be
  meaningful.

## Do not re-add it without

A pair of tests proving it fires on a known hole **and** stays silent on every
registered scene, at every LOD. The second half is the one that failed.
