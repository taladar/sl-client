---
id: viewer-straddling-transparency-oit
title: A translucent prim straddling the waterline loses its emergent half
topic: viewer
status: bugs
origin: follow-up from viewer-particle-water-ordering (2026-07-30)
refs: [viewer-particle-water-ordering]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-particle-water-ordering]] fixed translucent-vs-water ordering by
bucketing every `Transparent3d` item **below → water → above** by its centre
height and making the water write depth. That is per-pixel correct for content
clearly above or below the surface, and the water depth-write refines the
above-water bucket per pixel (so a fountain's spray is occluded correctly where
it dips behind the surface).

What it does **not** solve: a single *translucent prim* (or mesh) that
**straddles the waterline** is bucketed whole by its centre — so its submerged
half and its emergent half both land in one bucket, and one of them is on the
wrong side of the water.

**It does not merely order wrong: the emergent half disappears** (measured on
the local grid, 2026-08-30). A translucent surface writes no depth, so a half
that lands in the *pre-water* bucket is drawn before the sea and the sea —
opaque, depth-writing — then paints straight over it. On a 3.30 m box centred
0.49 m under the surface (North Region, plywood at 50 % transparency), the
1.16 m standing above the water is simply absent: a near-horizontal capture
shows sea directly above *and* below a 24-pixel slab, which is the top face seen
edge-on, and nothing of the sides.

**And this is not reference parity** — the earlier note here was wrong about
that. The reference draws the same faces in **both** alpha pools, each clipped
per fragment against the water plane (`lldrawpoolalpha.cpp`'s `waterSign` and
`WATER_WATERPLANE`, with the sign flipped when the eye is submerged), so every
fragment lands in the right pool whatever its object's centre is and nothing is
ever lost. The per-object bucket is this port's own simplification.

A genuinely per-pixel fix needs one of:

- **Clip-plane double-draw.** Draw each translucent object twice against the
  water plane — once clipping to below-water fragments (pre-water bucket), once
  to above-water (post-water bucket) — so every fragment lands in the right
  bucket. Per-pixel exact, but doubles transparent draw calls and needs a
  clip-plane uniform threaded into every translucent material (Bevy's
  `StandardMaterial` has no clip plane, so this touches the shared face
  material).
- **Order-independent transparency (OIT).** Weighted-blended OIT or a
  per-pixel linked list removes the ordering problem for *all*
  translucent-vs-translucent cases at once (not just water), but WBOIT has its
  own accuracy trade-offs and depth-peeling is expensive. This is a whole
  render-architecture effort in its own right.

Of the two, the **clip-plane double-draw is the reference's own answer** and the
one to port: the sign it clips by is already computed per view by
`classify_bucket` ([[viewer-underwater-translucent-drawn-behind-surface]]), so
what is missing is the plane uniform on the shared face material and drawing the
translucent phase twice against it.

## Reproduction status

`SCENES`' `water-straddling-translucent-prim` (a half-transparent emissive box
resting 0.5 m under the surface, seen from above against open sea) and the
readback check `a_translucent_prim_standing_out_of_the_water_is_drawn` were
added for this — and **the synthetic scene does not yet reproduce it**: the
emergent band renders there. So the check is a true invariant that does not yet
hit the failing configuration, and what separates the two setups is not yet
known. Finding it is [[viewer-water-transparency-scene-matrix]], which walks the
combinations exhaustively instead of guessing at one of them.
