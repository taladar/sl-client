---
id: viewer-water-transparency-scene-matrix
title: Walk the translucent-vs-water combinations exhaustively in the readback tier
topic: viewer
status: done
origin: user request while verifying viewer-straddling-transparency-oit (2026-08-30)
refs:
  [
    viewer-straddling-transparency-oit,
    viewer-underwater-translucent-drawn-behind-surface,
    viewer-translucent-top-face-reads-opaque,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Translucency against the water surface has now produced three separate reports
in two days, and each was chased by a human logging in, framing a cube, and
describing what they saw. The space is small, finite and entirely synthetic —
a box, a plane, and a camera — so a machine should walk it.

## The axes

Every combination of:

- **Object centre** relative to the water level: below, exactly at, above.
- **Face centre** relative to it: below, at, above. Not implied by the object's
  — a box centred *on* the waterline has its four side faces centred on it too,
  while its caps sit clear on either side, and `classify_bucket` reads the
  **face** centre (Bevy's `TransparentSortingInfo3d::Sorted::mesh_center`, the
  face mesh's world-space AABB centre).
- **Camera** relative to it: below the surface, at it, above it. The bucket
  flips with the eye ([[viewer-underwater-translucent-drawn-behind-surface]]),
  so this axis doubles every cell rather than decorating it.
- **What is behind the face**: open sea, sky, and an opaque marker. This one is
  load-bearing and was the trap the first attempt fell into — an emergent half
  silhouetted against the **sky** has nothing drawn after it and survives
  however it was bucketed, so the scene rendered correctly and proved nothing.
  Only a background of *sea* can show the depth-writing surface painting over
  it, and only an opaque marker can prove a face is genuinely translucent (the
  marker's colour reaching the frame through it) rather than merely bright.

## What each cell asserts

Two questions, both decidable from a pixel and neither needing a golden image:

1. **Is the face drawn at all** where it should be — its own strongly-coloured,
   emissive marker colour present in the band it occupies.
2. **Is it translucent** — the colour of whatever stands behind it reaching the
   frame through it.

## Where it goes

`sl-viewer-world-scene`'s `SCENES` and the viewer's `render_readback` tier,
which already has the machinery: a headless GPU capture, world points projected
through the very camera that drew the frame, and — since
[[viewer-underwater-translucent-drawn-behind-surface]] — the real
`TransparencyOrderPlugin`, without which none of this ordering was exercised at
all. `water-straddling-translucent-prim` and
`a_translucent_prim_standing_out_of_the_water_is_drawn` are the first cell.

A capture costs a few seconds, so the matrix should **not** be one scene per
cell: put several prims at different heights in one scene and let the camera
axis pick the scenes, then assert per prim per band from the one frame.

## Built (2026-08-30), and what it found

Six scenes — `water-translucency-{under,grazing,over}-{sea,backdrop}` — each
walking five boxes across the waterline in half-height steps, and two readback
checks that classify every band of every box: `translucent` (the prim's colour
and the backdrop's both present), `solid` (only the prim's), `missing` (only
what is behind it), `background` (neither). 39 cells per scene.

**The pipeline is correct on every axis the matrix walks.**

- Over an **opaque backdrop**, every face at every eye height comes back
  `translucent` — drawn, *and* see-through. That includes every **top cap**,
  which is what [[viewer-translucent-top-face-reads-opaque]] is about: a
  half-transparent cap does blend, from above the surface and from under it.
- Over **open sea**, every face is on screen, from all three eye heights.

**So the live defects are not explained by these axes**, and that is the useful
half of the result. [[viewer-straddling-transparency-oit]] is measured on the
grid — the emergent half of a prim resting mostly submerged is absent — and the
`sunk` box is exactly that shape, over open sea, from an eye above it. It
renders. Whatever separates the two is *not* the object's height, the face's
height, the eye's side of the surface, or what stands behind: it is something
the synthetic scene does not have. What is left to try, in rough order of
suspicion: the terrain under the sea, which the fixture has none of; the
underwater-fog / glow / tone-map passes the readback rig does not run; the sea
drawn as many 256 m region cells rather than one plane; and what remains of the
material difference — a decoded diffuse texture, and lighting rather than the
fixture's emissive (an emissive face reads as itself from any sliver, a lit one
may not), plus the cross-instance interning a live face goes through.

Note what is **not** a difference, which the first draft of this got wrong: the
fixture's faces are `FaceMaterial`s, not bare `StandardMaterial`s —
`spawn_geometry` wraps every fixture material in `inert_face_material` — and a
plain live diffuse face is *also* an inert-extension `FaceMaterial`
(`compose_face_material` builds one and sets only `glow`). The extension is
identical on both sides for a face carrying no legacy material.

A **fourth** lesson came from the hooks rather than from a run: a band on the
far side of the surface from the eye is seen *through* it, so what reaches the
frame is a refracted sample displaced by the wave normal — and the waves animate
from `globals.time`, so a shallow cell lands somewhere slightly different every
run. `grazing: sunk below` passed standalone and failed under the loaded
parallel `nextest` the pre-commit hook runs. Such a cell is now reported and not
asserted below a slope threshold (`REFRACTED_MIN_SLOPE`), which is where the
displacement stops being small against the band. The tier already had one
load-sensitive check ([[viewer-render-readback-texture-anim-test-flaky]]); this
one is not allowed to become the second.

Three lessons the fixture had to be taught from its own runs, all recorded in
it: a box centred
exactly on the waterline has its four side faces centred there too, which is not
the case in the wild; and a face silhouetted against the **sky** has nothing
drawn after it, so it survives however it was bucketed and proves nothing. A
third came out of the first run: over a *bright* background a drawn face carries
the backdrop's channel anyway, so "drawn" and "see-through" cannot be told apart
by one threshold — the sea checks accept both verdicts and only the backdrop
scenes decide translucency.

## Why it is worth the build

[[viewer-straddling-transparency-oit]] is a live, measured defect that the first
synthetic scene does **not** reproduce, and nobody knows yet which of these axes
separates the two setups. The matrix answers that as a by-product of existing,
and then guards the clip-plane fix that follows.
