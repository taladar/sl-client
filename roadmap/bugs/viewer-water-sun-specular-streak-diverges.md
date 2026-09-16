---
id: viewer-water-sun-specular-streak-diverges
title: The sun's specular streak on the water is narrow and white, not wide
  and orange
topic: viewer
status: bugs
origin: seen in the frames that settled
  [[viewer-sky-sunset-preset-glow-divergence]] (2026-09-16)
refs: [viewer-sky-sunset-preset-glow-divergence]
---

Context: [context/viewer.md](../context/viewer.md).

With the sky itself now matching Firestorm at every day position, one
difference is left in the same frames: the **sun's specular reflection on the
water**.

`sl-crosscheck --scenario catalogue --day-position 0.75 --camera-position
4,128,40 --camera-look-at=-2000,128,36`, the streak below the setting sun:

- **sl-client:** a narrow, near-vertical line, cool white shading to pale
  green-blue, roughly a fifth the width of the reference's and reaching further
  down the frame.
- **Firestorm:** a wide warm orange plume, brightest just under the horizon and
  fading out well before the bottom of the frame.

Both viewers agree about the sky above it, about the sun's direction, and about
the water frame's name — so this is the water surface's own shading, not the
light it is given.

## Where to look

The reference's specular light for water is **not** the atmospheric
`sunlight_color` uniform. `lldrawpoolwater.cpp` binds the frame's raw colour:

```cpp
LLColor4 specular(sun_up ? psky->getSunlightColor()
                         : psky->getMoonlightColor());
```

and separately builds a `light_diffuse` it *normalises* and then scales by the
light's ground projection ("Apply magic numbers translating light direction into
intensities"):

```cpp
light_dir.normalize();
F32 ground_proj_sq = light_dir.mV[0] * light_dir.mV[0]
                   + light_dir.mV[1] * light_dir.mV[1];
if (0.f < light_diffuse.normalize())
{
    light_diffuse *= (1.5f + (6.f * ground_proj_sq));
}
```

That `1.5 + 6·ground_proj²` term peaks at exactly the low sun this frame has, so
it is the first thing to check against `water.rs` / `water.wgsl` — along with
which of the two colours each of our specular terms is fed, since the hue is
wrong as well as the width.

Note the sky fix deliberately did **not** touch the water's `sunlight_color`
(see the sibling task): that uniform is the raw authored colour on both sides,
and normalising it here would be the wrong port.

## How to verify

Re-run the command above at day positions 0.75 and 0.25 and compare the streak's
width, hue and vertical extent; `sl-crosscheck-report` ranks the tiles it
occupies.
