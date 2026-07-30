---
id: viewer-straddling-transparency-oit
title: Per-pixel ordering for translucent objects straddling the waterline
topic: viewer
status: ideas
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

What it does **not** solve: a single large *translucent prim* (or mesh) that
**straddles the waterline** is bucketed whole by its centre — so its submerged
half and its emergent half both land in one bucket, and one of them orders
wrong against the water. This is reference parity (`LLDrawPoolAlpha` classifies
per spatial-group, also not per-pixel), but the point of this viewer is to *not*
reproduce the reference's decades-old transparency artifacts.

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

Neither is in scope for the water-ordering bug; filed so the residual is
tracked rather than forgotten.
