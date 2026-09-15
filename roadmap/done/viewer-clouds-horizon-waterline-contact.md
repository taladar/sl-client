---
id: viewer-clouds-horizon-waterline-contact
title: Check clouds vs the waterline at the horizon against Firestorm
topic: viewer
status: done
origin: split from viewer-clouds-sun-occlusion-horizon-contact (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact,
  viewer-sky-sunset-preset-glow-divergence,
  viewer-cloud-noise-scale-divergence]
---

Context: [context/viewer.md](../context/viewer.md).

**Resolved 2026-09-15.** Faithful to within a degree — see the A/B below. On
the way it fixed nil sun / moon / cloud ids drawing the built-in texture, and
ported the per-vertex horizon fade.

Split out of [[viewer-clouds-sun-occlusion-horizon-contact]] (the "clouds
touch the water in the distance" half). The other half of that ticket
turned out to be a sky colour/HDR problem, not a cloud-geometry one — see
that task for the `srgb_to_linear` fix and the bloom follow-up.

**Status: could not reproduce this session.** On a fresh look (local
OpenSim + aditi, 2026-08-03) the clouds **faded out above the waterline**
rather than touching it — the *opposite* of the original report. The cloud
dome geometry, `cam_height` (`0.96 × 15000 = 14400`), and the
`altitude_blend_factor` horizon fade were all re-verified faithful to the
reference (`buildStripsBuffer` + `renderDome` + `getCamHeight`,
`cloudsF.glsl`): clouds fade to zero as `rel_pos.y → 0` (the horizon) and
`SL-11589` clamps them off below it, so a faithful port should not droop
clouds onto the water.

**What this task is:** confirm the correct behaviour against Firestorm on
the *same* sky and decide whether anything is actually wrong:

- If Firestorm also fades clouds out above the waterline → close this as
  faithful (our current behaviour already matches).
- If Firestorm's clouds visibly reach the waterline → our fade clamps too
  high; candidates are the `altitude_blend_factor` ramp
  (`(rel_pos.y + 512) / max_y`), the dome cap's lower rim angle at our
  camera heights, or fog-over-water hiding the reference's rim where ours
  shows it.

Now easy to reproduce: `SL_VIEWER_SKY_DAY_POSITION` moves the sun and
`--camera-position` / `--camera-look-at` aim correctly (both fixed
2026-08-03), so frame a low sun over open water (aim west/`−X` at sunset)
and A/B against Firestorm on the same frame before changing the port.

## A/B against Firestorm (2026-09-15)

Run with `sl-crosscheck` on the fake grid's `catalogue` scene, both viewers
pinned to the `sky-sunset` legacy preset (`--day-position 0.75`), camera at
`4,128,40` looking west over the void water at `-2000,128,36`.

### The first A/B could not answer the question: a nil `cloud_id`

Firestorm drew **no clouds at all**; sl-client drew them. The fake grid's
presets are built on `SkySettings::legacy_windlight_default`, which left
`cloud_id` unset, so the wire carried the nil id — and the two viewers read a
nil sun, moon or cloud id differently:

- **The reference draws nothing.** `LLVOSky::setCloudNoiseTextures` binds no
  noise for a nil id and `renderSkyCloudsDeferred` skips the pass without one;
  `renderHeavenlyBodies` draws the sun disc "if and only if we have a texture
  defined", and the moon likewise.
- **sl-client drew the built-in texture** (`unwrap_or(DEFAULT_*_TEXTURE)` in
  `sky.rs`), so a nil id put a disc or clouds in the sky that the reference
  leaves empty.

This is not only a fake-grid artefact. The reference's own default sky names no
sun texture (`GetDefaultSunTextureId` is null), and neither does **OpenSim's**
(`ViewerSky.cs`: `sun_id = UUID.Zero`) — so on the local grid sl-client drew a
sun disc Firestorm does not. And the reference's `defaults()` *does* name the
built-in moon and cloud noise, which our legacy default did not.

Fixed:

- `sky.rs`: a disc is drawn only when its body is up **and** the frame names a
  texture (`disc_drawn`); the cloud dome is hidden when the frame names no
  noise. Nothing is fetched for an unnamed texture.
- `SkySettings::legacy_windlight_default` mirrors `defaults()`: no sun texture,
  the built-in moon and cloud noise.
- The editor's texture knobs: an unset sun / moon / cloud image reads as the nil
  id (an empty swatch — the reference's texture control shows none for a nil
  id), and the picker's **None** clears the field instead of storing
  `Some(nil)`. Bloom, halo, rainbow and the water textures keep "unset = the
  built-in default", which is what the reference renders for those.
- Docs on the three fields and constants (`sl-proto`), the RLV setter, and the
  fake-grid book chapter.

### The waterline itself: faithful to within a degree

With clouds on both sides, the contact question comes down to one line of
`cloudsV.glsl`: `altitude_blend_factor` is `(rel_pos.y + 512) / max_y`, cut to
zero for a dome vertex below the camera. It is evaluated **per vertex** and
interpolated, so the reference fades clouds out across the one ring of dome
triangles that straddles the horizon — at the feature table's `WLSkyDetail`
of 96 (the value the harness Firestorm ran with; 128 on the top class) that
ring spans about half a degree, from ~35 % opacity just above the horizon to
nothing ~0.75° below it. So the reference *does* let clouds reach the
waterline, faintly.

sl-client evaluated the same factor per fragment on a 32-stack dome — a hard
cut rather than a fade, at the same height. Ported faithfully anyway:
`clouds.wgsl` now computes it in the vertex stage and interpolates it, and the
dome is 96 stacks × 192 slices (`the_horizon_fade_band_is_under_a_degree`
fails on the old 32). The re-captured frame differs by at most 5/255 in a few
faint cloud edges — the change is real but sub-degree, which is the point: the
two viewers agree about the waterline.

### What the frames show that is not this ticket

- The sunset sky itself differs a lot: sl-client's is bright and blue-topped
  with a glow filling a third of the frame, Firestorm's is dim grey-blue with a
  compact glow — [[viewer-sky-sunset-preset-glow-divergence]].
- The cloud blobs differ in size and count on the same stand-in noise: fewer,
  larger, horizontally stretched blobs here; more, smaller ones that shrink
  toward the horizon there — [[viewer-cloud-noise-scale-divergence]].
- Firestorm's void water meets the sky ~0.7° (13 px) lower than the geometric
  horizon sl-client draws. Its harness runs at a 128 m draw distance against
  our 512 m, so this is not filed as a divergence without a same-draw-distance
  capture.
