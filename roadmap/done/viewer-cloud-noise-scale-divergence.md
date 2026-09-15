---
id: viewer-cloud-noise-scale-divergence
title: Clouds come out fewer and larger than Firestorm's on the same noise
topic: viewer
status: done
origin: found A/B-ing [[viewer-clouds-horizon-waterline-contact]]
  (2026-09-15)
refs: [viewer-clouds-horizon-waterline-contact]
---

Context: [context/viewer.md](../context/viewer.md).

Same crosscheck as [[viewer-clouds-horizon-waterline-contact]] (fake grid,
`sky-sunset`, looking west over water), both viewers sampling the fake grid's
stand-in cloud noise (`sl_test_assets::builtin::cloud_noise`, one raised-cosine
blob per tile):

- **sl-client:** about eight visible blobs, ~100 px wide, stretched
  horizontally, keeping their size down to the horizon.
- **Firestorm:** about sixteen, ~40–70 px, shrinking toward the horizon as
  perspective would suggest, and visible right up to the top of the frame.

The cloud positions themselves are not comparable (each viewer's scroll starts
at its own arrival), but size and count are.

What is already the same: the dome geometry and its baked planar UV
(`buildStripsBuffer`), `cloud_scale` handling, the octave arithmetic of
`cloudsF.glsl`, and — since the waterline ticket — the per-vertex altitude fade
and the 96-stack dome.

Suspects, in order:

- **Sampling of the 16× detail octave.** `uv3 = uv * 16` is sampled with the
  reference's mipmapped `GL_RGBA8` texture; ours uploads the noise without
  mips (`cloud_noise_image`), so far-away texels alias instead of averaging,
  and which octave clears the density threshold changes.
- **The stand-in noise itself.** A single blob per tile is a pathological
  input: nearly all of it is zero, so `alpha1` hinges on thresholds that real
  noise never approaches. Worth repeating on aditi, where both viewers fetch
  Linden's own noise, before concluding anything from the fake grid.

## How to verify

Repeat the crosscheck, and an aditi side-by-side on the same EEP sky; blob
count and size should agree.

## Resolution (2026-09-15)

**The cloud layer was turned a quarter-turn about the zenith.** Neither
suspect held: the lattice spacing was identical all along, and blob size came
from where the lattice sat, not from how it was sampled.

`renderDome` rotates the WindLight dome 120° about `(1, 1, 1)` before drawing
it, so the frame `buildStripsBuffer` bakes its planar texcoord in is
**x north, y up, z east** — and `toLightNorm` hands the shader its light
direction in the same frame. `build_cloud_dome_mesh` took the same
`((-z0 + 1) / 2, (-x0 + 1) / 2)` from Bevy's **(east, up, south)**, and
`clouds.wgsl` offset the self-shadow layer by Bevy's `lightnorm.x/.z`. So
the whole layer sat on other ground, and its west-east scroll carried clouds
north-south.

Why it read as "fewer, larger": the visible blobs are the 16× detail octave's
peaks, gated by the 1× octave (`alpha1 > 0` needs `n1 + n3 > 0.96`). A
lattice laid over a different stretch of the 1× octave clears the threshold
by a different margin, so the blobs grow, shrink and drop out together.

### How it was pinned down

- A NumPy model of the reference projection (camera ray → dome cap →
  baked UV → `uv3` octave) reproduces the positions of Firestorm's blobs to
  within a few pixels. Built in Bevy's frame, the same model reproduces
  ours; built in the reference's, it does not.
- The first capture's sl-client camera had also yawed 18° off the pose it was
  given. That was live SpaceNavigator/mouse input reaching the flycam (see the
  `screenshot-runs-eat-live-input` note), not a viewer defect, and a re-run held
  the pose.
- Mip-mapping cannot matter at this framing: both octaves are *magnified* on
  screen (tens to hundreds of pixels per noise texel), so no mip level is
  sampled.

### Fix

- `build_cloud_dome_mesh` builds the unit direction in the reference's dome
  frame and writes the vertex as Bevy `(z0, y0, -x0)`, so the baked texcoord
  is the reference's in world terms: `u` against east, `v` against north.
- `clouds.wgsl` converts the Bevy light direction to the dome frame for the
  self-shadow offset (`uv1v += (north, east) × 0.0125`). The other uses of
  `lightnorm` are dot products and the up component, which are the same in
  either frame.
- `the_cloud_texture_axes_follow_east_and_north` pins the texcoord to the
  world axes (and fails on the old mapping).

### Verified

Crosscheck, fake grid `catalogue`, `sky-sunset` (`--day-position 0.75`),
camera `4,128,40` → `-2000,128,36`: after the fix, sl-client's blobs sit on
Firestorm's (e.g. the top row at x ≈ 230 / 730 / 1235 px of 1440 in both),
at the same small size, down to the horizon rows.

The aditi side-by-side on Linden's own noise was **not** run: the viewer was
launched there, but a Firestorm comparison was not available at the time, and
the axis mapping was accepted from the reference sources and the fake-grid A/B.
