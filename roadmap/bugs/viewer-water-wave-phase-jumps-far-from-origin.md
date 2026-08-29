---
id: viewer-water-wave-phase-jumps-far-from-origin
title: The sea's ripple phase jumps instead of scrolling, worse the further from the origin
topic: viewer
status: bugs
origin: reported while reviewing viewer-water-surface-alpha-not-refraction (2026-08-29)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

The wave scroll on the water surface does not slide smoothly: the phase jumps
irregularly, above and below the surface alike. It happens **near the avatar
too**, and camming a region or two away makes it much worse. The reporter notes
the reference viewer shows the same thing, and for the same reason — so this is
one to fix rather than to port.

Not caused by the refraction work
([viewer-water-surface-alpha-not-refraction](../done/viewer-water-surface-alpha-not-refraction.md)),
which does not touch the wave normals; it was simply noticed while looking hard
at the water.

The cause is float precision in the texcoords, in
`sl-client-bevy/src/water.wgsl`:

```text
var v = horiz;                       // the fragment's ABSOLUTE world x/z
v.x += (cos(v.x * 0.08) + sin(v.y * 0.02)) * 6.0;
let little_wave_a = v * vec2(0.45, 0.9) + water.wave2_dir * wave_time * 0.13;
```

`horiz` is the fragment's world position, and the endless ocean is a 40 km plane
that follows the camera, so `v` reaches tens of thousands of metres. At
`v = 9000` an `f32` resolves about `1e-3`, while one frame of scroll at 60 fps
moves the texcoord about `0.13 * 0.016 = 2e-3` — two representable steps. The
scroll quantises, which reads exactly as an irregular jump rather than a slide,
and it gets worse as the coordinates get bigger: further from the scene origin,
and further from the camera within the same frame. The `cos(v.x * 0.08)` sweep
has the same problem in its argument.

The fix is camera-relative texcoords, the standard remedy: shade with
`(v - cam_horiz) * s + fract(cam_horiz * s)` per layer instead of `v * s`. The
first term is small wherever the water is actually being looked at, so it keeps
its precision; the second carries the absolute phase and is exact enough because
the *camera's* coordinate is small next to a horizon fragment's. The wave normal
map repeats, so subtracting a whole number of periods is invisible — that is
what makes the substitution legal.

`view.world_position` is already in the shader, so this needs no new uniform.

Check first whether it is still there. The ocean plane was one 40 km quad until
[viewer-water-surface-alpha-not-refraction](../done/viewer-water-surface-alpha-not-refraction.md)
subdivided it, and the interpolation error that fixed — a fragment's world
position, disagreed on across a triangle with an enormous `w` range — moves as
the camera moves. That is also a phase jump, from the other end of the same
pipeline, and it may have been most of this one.

Verify by camming out a region or two over open water and watching the ripples,
which is how it was found; a screenshot pair cannot show a phase jump.
