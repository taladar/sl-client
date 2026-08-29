---
id: viewer-sea-distance-band-hard-seam
title: The open sea has a hard ring at a fixed distance, light beyond it and dark inside
topic: viewer
status: bugs
origin: user report while verifying viewer-nametags-occluded-by-clouds (2026-08-29)
refs: [viewer-nametags-occluded-by-clouds]
---

Context: [context/viewer.md](../context/viewer.md).

Looking out over open water, the sea is split into two visibly different
surfaces by a **hard, camera-centred boundary** at a fixed distance: everything
beyond it is markedly lighter (close to the sky's own colour), everything inside
it is the normal darker sea. There is no gradient across the seam — it is a
step. Reported as "a visible dome that dissects the infinite ocean into a part
inside and outside it".

## What is established

- **Reproduced offline** on the local OpenSim with the screenshot harness, from
  Default Region looking west over the void:

  ```sh
  SL_VIEWER_SKY_DAY_POSITION=0.35 ./target/release/sl-client-bevy-viewer \
    --credentials credentials.toml --grid localhost --avatar primary \
    --camera-position 30,128,60 --camera-look-at -3000,128,10 \
    --screenshot-dir <dir>
  ```

  Sampling a column down the frame shows sky, then a flat light band
  (~`(150, 180, 212)`), then a one-sample step down to the ordinary sea
  (~`(100, 122, 147)`).

- **Not the transparent-phase ordering.** The same capture was taken with
  [[viewer-nametags-occluded-by-clouds]]'s backdrop bucket stashed out, and the
  seam is identical in the baseline build. It is also mechanically impossible
  for that change to cause it: the sea is `AlphaMode::Opaque` +
  `reads_view_transmission_texture`, so it renders in `Transmissive3d`, a phase
  drawn in full **before** the `Transparent3d` phase that change re-sorts.

- **Not a depth prepass.** There is no `DepthPrepass` in the viewer at all —
  `underwater_fog` deliberately samples the main-pass depth texture instead
  (a prepass would need depth pipelines for the custom sky / terrain / water
  materials, whose `specialize` pins bespoke vertex layouts).

- **Not the sea grid running out.** `SEA_GRID_RADIUS_CELLS = 17` with a
  Chebyshev `cell_distance`, so the nearest grid edge is at least
  `17 × 256 = 4352 m` — beyond the 4096 m far plane, and further out than the
  observed seam.

## Leading hypothesis: it is where a sky dome meets the water plane

The user's reading of it, and the arithmetic supports it. Every backdrop is
anchored to the camera, so each one cuts the water plane in a circle centred on
the viewpoint — exactly the shape seen. With the camera `h` metres above the
water:

| Backdrop | Geometry | Ring radius |
| --- | --- | --- |
| Sky dome | sphere, `SKY_DOME_RADIUS = 3000` | `sqrt(3000² − h²)` |
| Star field | sphere, `STAR_DOME_RADIUS = 2900` | `sqrt(2900² − h²)` |
| Cloud dome | `[0, π/8]` cap of a 15 km sphere whose centre is `CLOUD_DOME_RADIUS × CLOUD_DOME_OFFSET = 14400 m` below the camera | `15000 × sin φ`, `cos φ = (14400 − h) / 15000` |

At the `h = 40 m` of the repro capture that is **3000 m** (sky), **2900 m**
(stars) and **4335 m** (clouds). The measured seam sat near 2.9 km, which lands
on the two spheres and nowhere near the cloud cap — but see the caveat on that
measurement below. The sun / moon discs are 2 km billboards, not domes, so they
cannot draw a ring at all.

**What makes this a real puzzle rather than an obvious fix:** all three domes
force `clip_position.z = 0` — the reverse-Z far plane — in their vertex shaders,
precisely so real geometry at any altitude occludes them. The sea is opaque and
depth-writing. So a dome should be *incapable* of marking the water, and the
fact that a dome crossing is visible at all is itself the thing to explain.
Suspect the paths where that forcing does not apply: the sea samples the screen
copy (`fb` in `water.wgsl`) which the opaque pass has already painted the dome
into, and the reflection-probe capture cameras render the domes through views of
their own.

## Telling the candidates apart

Two cheap sweeps, neither of which needs new code:

- **Raise the camera.** The two spheres' rings *shrink* as `sqrt(R² − h²)`; the
  cloud cap's ring *grows*, and above `h ≈ 540 m` the cap no longer reaches the
  water at all (φ hits the `π/8` rim) so its ring vanishes outright. One
  `--camera-position` sweep separates "a sphere" from "the cloud cap".
- **Midday vs midnight.** The star field is `Visibility::Hidden` below
  `STAR_ALPHA_THRESHOLD`, so a ring that survives at midday is not the stars.

Then **measure the radius properly** rather than trusting the pixel estimate: it
leaned on an assumed vertical FOV, so park the camera at a known height and step
`--camera-position` until the seam crosses a known landmark.

If the domes are exonerated, the next candidates are:

- The **clustered-forward Z range**: the camera pins
  `ClusterFarZMode::Constant(512.0)` (`lib.rs`, for the facelight fix). If
  reflection-probe / environment-map assignment falls off the end of the cluster
  grid, water fragments past it would lose the probe reflection and fall back to
  `water.reflection_color` — a step exactly of this shape. See the
  `sl-client-local-light-rendering` memory for the cluster gotcha this config
  exists to work around.
- The water shader's own distance terms — `dmod`, the fresnel, and the
  `ENVIRONMENT_MAP` branch in `water.wgsl`.

Bisecting the recent sea work (`b2571952`, `a3beaf44`, `31bf6a30`) against an
older commit at the same fixed camera pose would say immediately whether this
arrived with them or predates them.
