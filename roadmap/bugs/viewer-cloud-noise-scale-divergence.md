---
id: viewer-cloud-noise-scale-divergence
title: Clouds come out fewer and larger than Firestorm's on the same noise
topic: viewer
status: bugs
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
