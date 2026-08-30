---
id: viewer-straddling-transparency-oit
title: A translucent prim straddling the waterline loses its emergent half
topic: viewer
status: done
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

## Fixed (2026-08-30), live-verified

Ported the reference's own answer: the same face is drawn **twice**, each draw
clipped per fragment to its own side of the water plane
(`sl-viewer-world-scene/src/water_clip.rs`).

- `SlFaceParams::water_clip` / `water_level` and a `waterClip` in
  `face_material.wgsl` — `0` = no clip, `+1` keeps the fragments above the
  surface, `-1` those below, the rest discarded. This is the reference's
  `waterSign` uniform and its `waterClip` (`deferredUtil.glsl`).
- The reference flips that uniform between its two **pool draws** of one list.
  Bevy cannot: a phase item carries one pipeline and one material binding, and
  the pass that draws it is not ours to parameterise. So the second draw is a
  **twin entity** parented to the face, sharing its mesh and carrying a copy of
  its material with the opposite sign. The face's material is copied first,
  because it may be interned across every identical face in the scene.
- `classify_bucket` buckets a clipped draw by **the side it keeps** rather than
  by its centre — both halves share one centre, which is why the centre could
  never decide this — and which side is the far one still flips with the eye.
- Only faces that actually straddle are split, decided from the world-space
  **extent** rather than the centre, and only alpha-blended ones (an opaque face
  writes depth and is already ordered per pixel). A face that stops straddling
  is made whole again.

**Verified on the grid**, same camera pose before and after: the band above the
waterline on the North Region box read as open sea (blue-dominant, `b≈147`)
before, and reads as plywood blended half-and-half over the water
(`≈(126,128,131)`) after. The emergent half is drawn, and it is see-through.

Guarded by `water_clip`'s own tests (a straddling translucent face is split, one
clear of the surface is not, an opaque one never is, one that leaves the water
is made whole, and the straddle test is on the extent not the centre) and by
`a_clipped_half_is_bucketed_by_the_side_it_keeps` in `transparency`. The
readback matrix ([[viewer-water-transparency-scene-matrix]]) now renders through
this path too, since its fixture faces carry the viewer's `PrimFaceEntity` tag.

`RUST_LOG=info,sl_viewer::water_clip=debug` names each face as it is split.

## Reproduction status (before the fix)

**Not reproduced synthetically**, and the axes anyone would have guessed at are
now ruled out. [[viewer-water-transparency-scene-matrix]] walks five boxes
across the waterline (including `sunk`, which is exactly this case: a prim
resting mostly submerged, its sides bucketed by a centre under the water while
their upper halves stand above it) from three eye heights over both open sea and
an opaque backdrop, classifying every band of every box. Every cell renders.

So it is **not** the object's height, the face's height, the eye's side of the
surface, or what stands behind — it is something the synthetic scene does not
have. In rough order of suspicion:

- the terrain under the sea, which the fixture has none of;
- the underwater-fog / glow / tone-map passes, which the readback rig does not
  run. (The glow pass is ruled out for the *top face* by a pinned-day-position
  live A/B — see [[viewer-translucent-top-face-reads-opaque]] — but **not** for
  the emergent side band, which that capture's viewpoint could not see.);
- the sea drawn as many 256 m region cells rather than one plane;
- what is left of the material difference: a decoded diffuse texture, and
  lighting rather than the fixture's emissive (an emissive face reads as itself
  from any sliver of coverage, a lit one may not), plus the cross-instance
  interning a live face goes through.

The **material type** is not among them, though a first draft of this said it
was: the fixture's faces are `FaceMaterial`s — `spawn_geometry` wraps every
fixture material in `inert_face_material` — and a plain live diffuse face is
also an inert-extension `FaceMaterial`, since `compose_face_material` builds one
and sets only `glow`.

The next step is to close the gap rather than guess again: add terrain under the
fixture's sea, then a lit rather than emissive face, and see which turns the
cell red.
