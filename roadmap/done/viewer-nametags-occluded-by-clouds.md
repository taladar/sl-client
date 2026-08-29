---
id: viewer-nametags-occluded-by-clouds
title: Name tags render behind the cloud layer (near tag occluded by far clouds)
topic: viewer
status: done
origin: user report during the GPU-avatar Phase 1a side-by-side (2026-08-13)
refs: [viewer-hover-tooltips]
---

Context: [context/viewer.md](../context/viewer.md).

An avatar name tag that is geometrically **in front** of the clouds (the avatar
is near; the clouds are the far sky) was **occluded by** them — the far cloud
layer painted over the nearer transparent billboard. Reported on Aditi/OpenSim;
independent of the GPU-avatar work (it is the *regular* avatar name tags, and
pre-existing).

## Cause (confirmed)

The suspected cause held. Bevy sorts the `Transparent3d` phase by each item's
**mesh centre** distance, and the cloud dome's mesh centre is the *camera*: the
reference's `getCamHeight` offset is baked into the dome vertices and the
visible cap is only the `[0, π/8]` crown of a 15 km sphere, so the mesh's AABB
centre sits ~29 m from the dome origin, which `center_sky_on_camera` pins to the
eye every frame. Sort distance ≈ 0 makes the dome the **nearest** transparent
object in the scene, so it is drawn **last** — over every world-anchored
transparent overlay in front of it. Name tags are depth-tested but do not write
depth (reference-correct, `LLGLDepthTest(GL_TRUE, GL_FALSE)`), so they cannot
depth-reject the later cloud draw even though the cloud fragments are at the far
clip plane. The star field is the same shape of problem, and the sun / moon
discs sort by a fixed 2000 m rather than as the backdrops they are.

`sky::tests::the_cloud_dome_mesh_is_centred_on_the_camera` pins that premise so
the fix does not silently stop being load-bearing.

## Fix

A **backdrop bucket** in the existing transparent-phase re-sort
(`sl-viewer-world-scene/src/transparency.rs`, the machinery water ordering
already uses). A new `SkyBackdrop` marker component (`HeavenlyBody`, `Stars`,
`Clouds`) is put on the sun / moon discs, the star field, and the cloud dome,
mirrored into the render world by `extract_sky_backdrops`, and read by
`sort_transparent_by_water`, whose key is now `(bucket, backdrop order,
distance)`:

- buckets ascend below-water → **backdrop** → above-water → always-on-top, so
  the backdrops draw before every world-anchored overlay (name tags, hover text,
  parcel borders, particles) and the pre-water pass still takes the below-water
  items as a prefix of the phase;
- within the bucket the order is the reference's own
  (`LLDrawPoolWLSky::renderDeferred`): heavenly bodies, then stars, then clouds
  — so the clouds still pass **in front of** the sun, which
  [[viewer-clouds-sun-occlusion-horizon-contact]] fixed.

The sky dome itself needed no marker: it is opaque and draws in the `Opaque3d`
phase, before any of this — which is why only the clouds showed the bug.

The backdrop test runs **before** the water test, which also fixes a latent
second bug: the cloud dome's and star field's mesh centre is the camera, so the
moment the camera dipped below the water they dropped into the below-water
bucket and were handed to the pre-water pass to be refracted by the sea.

## Follow-up (separate, not a regression)

The three offline scene hosts — the gallery (`render_gallery.rs`),
`render_test.rs`, and `render_readback.rs` — do **not** add
`TransparencyOrderPlugin`, so the sky scenes in `render_scene.rs` still order
their transparent content by plain distance, without either the water buckets or
the backdrop bucket. Nothing visibly breaks today (those scenes' only non-sky
content is opaque), but it is a real parity gap between the gallery and the
viewer, and adding the plugin there would change water ordering in every scene,
so it wants its own task and its own look.
