---
id: viewer-water-transparency-scene-matrix
title: Walk the translucent-vs-water combinations exhaustively in the readback tier
topic: viewer
status: ready
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

## Why it is worth the build

[[viewer-straddling-transparency-oit]] is a live, measured defect that the first
synthetic scene does **not** reproduce, and nobody knows yet which of these axes
separates the two setups. The matrix answers that as a by-product of existing,
and then guards the clip-plane fix that follows.
