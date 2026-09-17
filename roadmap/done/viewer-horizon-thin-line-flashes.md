---
id: viewer-horizon-thin-line-flashes
title: Flashes along a thin line near the horizon
topic: viewer
status: done
origin: aditi run while tracing viewer-own-avatar-facing-drifts-idle (2026-09-15)
refs:
  - viewer-clouds-horizon-waterline-contact
  - viewer-underwater-fog-background-flicker
---

Context: [context/viewer.md](../context/viewer.md).

Seen by the user on aditi during an unrelated run (the idle facing trace, on
`bugs2` at `8e274869` plus a logging-only diagnostic): **flashes in a thin
line near the horizon**. The camera was the ordinary third-person follow
camera, the avatar standing or turning in place.

What the run's log says about the scene, and nothing more is known:

- Region environment from the EEP cap: the day cycle
  `3 hr day / 1 hr night #4` (8 sky frames, 1 water frame, day length
  14400 s, offset 57600 s).
- Draw distance 512 m.

Not known yet, and worth pinning down on the next sighting: whether the line is
**at the waterline** (the water plane meeting the sky dome) or at a **land
horizon**, its colour (a bright sky/sun colour, black, or a water colour), and
whether the flashes follow **camera rotation** (a per-pixel depth or precision
fight that moves with the view) or come on their own (a per-frame pass that
alternates, like the background flicker behind the underwater fog did).

## Suspects

- **The water plane's far edge against the sky dome**: a thin band where
  the two meet at grazing angles, where depth precision is worst and the
  water shader's reflection and fog both change fastest.
- **The cloud dome's horizon fade** ([[viewer-clouds-horizon-waterline-contact]]
  is about clouds at the same line, though that report is about geometry, not
  flashing).
- **Distant terrain or region-edge geometry** at the far plane, flickering
  in and out of the frustum or the draw distance.

## Capture next time

A screenshot sequence at a fixed pose, so frames can be diffed, rather than
the live window: `--camera-position` / `--camera-look-at` aimed at the
horizon with `SL_VIEWER_SCREENSHOT_{DELAY,INTERVAL,FRAMES}` set to catch
several consecutive frames. If a flash lands between two otherwise identical
frames, crop that line at full resolution. `SL_VIEWER_DISABLE_UNDERWATER_FOG=1`
is the cheap A/B for the fog pass.

## Fixed (2026-09-17)

It was a **dark** line, one to a few pixels tall, right where the sea meets the
sky. It flashed where its height changed from frame to frame. It looked the same
at a low sun and at *modern midday*, in every direction, and its colour did not
change with the time of day. That ruled out the sun's reflection on the water.
It was the deep-water fog colour, laid by the **water-haze pass**
(`underwater_fog.wgsl`) over sky that is not under the water.

Reproduced offline with `sl-crosscheck` on the `catalogue` scene: the camera at
`4,128,22`, looking west at `-2000,128,22`, day position 0.5. At that pose the
row where our sea meets the sky read `(87,107,122)`, against `(121,144,158)` for
the sea under it. Firestorm's matching row is a pixel *brighter* than its sea. A
temporary per-column split of the water shader's terms showed the dark samples
were not from the water surface. They were the fogged sky behind it.

Two halves, both in how the pass treats a pixel with an **empty** depth (only
sky), whose position it places on the view ray at the camera's far clip:

- **A tolerance meant for positions rebuilt from the depth buffer.** The clip
  let a point up to a thousandth of its distance above the surface through, so
  that far water pixels do not fall either side of the test. At the 4096 m far
  clip, that is four metres, which is more than an avatar's eye height over the
  sea. So sky rays up to just *above* the horizon got fogged. The position of an
  empty pixel comes straight from the ray and is exact, so it gets no tolerance
  now.
- **One decision per pixel, blended onto every multisample.** The sea's far edge
  crosses a pixel part-way. The pass fogged such a pixel from its centre, so the
  samples the water does not cover went dark. An empty pixel is now tested at
  its **highest corner**, so it is fogged only when all of it is under the
  surface. The reference renders without MSAA and shows sky there. So does this
  viewer now: that row reads `(180,210,230)`, a sky/sea blend.

The line's height followed the camera's height above the water, so an idle
avatar's bob made it flash. The depth-buffer case keeps its tolerance unchanged.

The first suspect was wrong, and so was the second: the sun-highlight port in
`612d28ca` came after the sighting, but the line was still there on a build
that had it.

### Verified

- **Offline:** the dark-edge measure (the first pixel down each column that is
  darker than the sea four rows under it) found a dark edge in 480 of 480
  sampled columns on the old build. On the fixed build it found none, at four
  poses: 21.4 m, 22 m and 23.7 m eye heights, looking west, south-west and
  south.
- **Regression test:** `the_water_haze_fogs_no_sky_above_the_sea` in
  `render_readback.rs` renders the haze pass alone over an empty stage. The eye
  is 0.3 m over the sea, and the field of view is narrow enough that one row is
  a metre at the far clip. Every row whose top edge reaches the far clip above
  the surface must keep the clear colour, and every row well under it must be
  fogged. On the old shader it fails on rows 124–128. With only the tolerance
  removed, it still fails on row 128, the half-covered edge row. The haze
  plugin now registers the `WaterLevel` it reads, so it runs without an ocean.
- **Live on aditi:** the user confirmed the dark line is gone at the same spot
  with the same low sun.

While checking it live, the user noticed an unrelated sea-level bug at the same
spot, filed as [[viewer-void-water-diagonal-takes-corner-height]].
