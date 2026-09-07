---
id: viewer-water-refraction-smears-avatar-silhouette
title: Water behind an avatar smears a skin-coloured fringe around its silhouette
topic: viewer
status: bugs
origin: seen on aditi while live-checking the water-clip skin fix (2026-09-07)
refs: [viewer-water-surface-alpha-not-refraction, viewer-water-exclusion,
  viewer-water-transparency-scene-matrix]
---

Context: [context/viewer.md](../context/viewer.md).

Wherever open water sits **behind** an avatar, a ragged skin-coloured fringe
hugs the avatar's silhouette — along the arms and shoulders most visibly. It
follows the outline exactly, is the colour of whatever the avatar's edge pixels
are (skin on a bare arm, white on a light top), and appears against water only:
the same silhouette against terrain or sky is clean.

The distance to the water does not matter. It shows with the sea a long way
behind the avatar, which is the tell that this is not geometry, not the
avatar's own shading and not the water surface being in the wrong place.

## The mechanism

`sl-client-bevy/src/water.wgsl` refracts by sampling Bevy's
`view_transmission_texture` — a **copy of the screen** holding the opaque scene
— at a UV the wave normal displaces:

```wgsl
let under_distort = clamp(
    screen_uv + vec2<f32>(wavef.x, wavef.z) * water.ref_scale,
    ...
textureSample(
    view_bindings::view_transmission_texture,
    view_bindings::view_transmission_sampler,
    under_distort,
```

The avatar is opaque, so it is **in that copy**. A water fragment just outside
the avatar's silhouette therefore samples, at the displaced UV, a texel from
*inside* the silhouette — and paints the avatar's colour onto the water. The
displacement is a screen-space offset, so it does not care how far away the
water is; it only cares that the avatar is adjacent in screen space. The
fringe's raggedness is the wave normal varying along the edge.

This is the classic screen-space-refraction artifact, and the reference viewer
does not have it because its refraction sample is taken from a screen copy that
excludes what is *in front of* the refracting surface.

## What to check first

1. Whether the fringe tracks `water.ref_scale` — turn it down and the fringe
   should narrow proportionally. That is the cheapest confirmation of the
   mechanism above, and it needs no grid: any translucent surface with an
   opaque object in front of it will do.
2. What the reference actually does about it. `lldrawpoolwater.cpp` /
   `water.glsl` clamp the distorted sample against the *depth* of the fragment
   being shaded — a sample whose depth is nearer than the water surface is
   rejected and the undistorted sample used instead. That is the standard fix
   and it needs the depth prepass texture, which the viewer already has.
3. Whether [`viewer-water-exclusion`]'s screen-space mask can be reused. It
   already marks where water renders; the question is whether it (or a cheap
   depth comparison) can also answer "is the texel I am about to sample in
   front of me".

## Why it matters

It is on every shoreline shot with an avatar, which is most of what anyone
photographs in Second Life, and it reads as a rendering fault rather than a
stylistic difference. It is also a correctness bug in the refraction itself:
the water is showing something that is not behind it.

Reference (Firestorm, read-only): `lldrawpoolwater.cpp`, `water.glsl` /
`waterF.glsl` (the distorted `screenTex` fetch and its depth rejection).
