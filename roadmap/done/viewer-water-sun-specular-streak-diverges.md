---
id: viewer-water-sun-specular-streak-diverges
title: The sun's specular streak on the water is narrow and white, not wide
  and orange
topic: viewer
status: done
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
`sunlight_color` uniform. `lldrawpoolwater.cpp` builds a `light_diffuse` it
*normalises* and then scales by the light's ground projection ("Apply magic
numbers translating light direction into intensities"):

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

## ROOT CAUSE (2026-09-16): three halves of one port, all absent

The specular was a **Blinn-Phong stand-in** for the reference's `pbrPunctual`,
said so in its own comment, and diverged from it in three independent ways. Each
is enough on its own to make a wrong streak; together they make this one.

**The shading normal is not the wave normal.** The reference flattens it toward
the vertical with distance:

```glsl
pbrPunctual(..., normalize(wavef + up*max(dist, 32.0)/32.0*(1.0-vdu)), ...)
```

so water past 32 m mirrors the sun as a *sheet*. Ours shaded on the raw wave
normal at every range, scattering the highlight off every wavelet.

**The lobe is a clamped plateau, not a falloff.** `pbrPunctual` is GGX, and its
`F·G·D / (4·N·L·N·V)` runs far past the `clamp(…, 0, 10)` the reference caps it
at over a wide band of a grazing view — the plume's *width is the width of the
saturated band*. A `pow(N·H, 370)` lobe has no such plateau.

**The colour was the frame's raw sunlight.** Two things replace it: the base
colour is the pool's unit-normalised `light_diffuse` above, so a legacy sunset's
`sunlight_color` of `2.8386` never reaches the shader as a magnitude; and the
result is scaled by the atmospheric `sunlit` of `calcAtmosphericVarsLinear`,
which is orange at dusk because it is the sun seen through a whole atmosphere.
Ours multiplied the raw `2.8386` by `max(light_dir.y, 0)` and clamped — a
colourless line.

Two smaller findings on the way:

- The sibling task's note that `lldrawpoolwater.cpp` "binds the frame's raw
  colour as its specular" is **wrong**. The `LLColor4 specular(...)` at line 192
  is dead — declared, never read. `uniform1f(WATER_SPECULAR, …)` is handed
  `light_diffuse`.
- The water program declares `calculatesAtmospherics` too, so `syncLightState`
  overwrites *its* `sunlight_color` in classic mode as it does the sky's.
  The atmospheric `sunlit` the fix feeds it therefore reads the colours off the
  resolved `SkyParams`, not off the frame.
- `WATER_BLUR_MULTIPLIER` is bound **doubled** (`max(0, blur) * 2`) and is the
  shader's `perceptualRoughness`. The doubling now lives at the binding rather
  than in the one place downstream that remembered it.

## Verified (2026-09-16)

`sl-crosscheck --scenario catalogue --camera-position 4,128,40
--camera-look-at=-2000,128,36`, frame 29, against the same Firestorm frames.

At `--day-position 0.75` the streak's peak colour, measured at rows below the
horizon (`sl-client` before → after, against the reference):

| row below horizon | before | after | Firestorm |
| --- | --- | --- | --- |
| 20 | `192,184,150` | `255,255,174` | `235,235,151` |
| 200 | `121,116,114` | `255,255,198` | `251,245,147` |
| 300 | `61,66,76` | `188,133,92` | `144,105,81` |
| 450 | `26,40,57` (absent) | `64,55,60` | `54,50,59` |

Before, the streak was colourless (blue-shifted by row 300: `B−R` **+15**) and
gone by row 400. After, it is warm along its whole length (`B−R` −81…−33 against
the reference's −84…−14), and its core width tracks the reference (42 vs 48 px
at row 20, 59 vs 53 at row 200, 131 vs 131 at row 400).

Whole-frame mean |diff| against Firestorm **0.0186 → 0.0145**; over the sea band
alone **0.0199 → 0.0113**, a 43% reduction. The off-streak sea agreed to within
2/255 before and after, so what moved is the highlight and nothing else.

At `--day-position 0.25` the sun is behind this camera and neither viewer draws
a streak; both seas agree, so the change adds no highlight where the reference
has none.

**Residual**: over the nearest water (rows 400–500) ours runs about 10–12/255
brighter than the reference's. Not filed separately — it is within the same
band as the sky's own settled residual, and the frames agree on width, hue and
extent.
