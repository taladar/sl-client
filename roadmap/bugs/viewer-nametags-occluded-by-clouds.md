---
id: viewer-nametags-occluded-by-clouds
title: Name tags render behind the cloud layer (near tag occluded by far clouds)
topic: viewer
status: bugs
origin: user report during the GPU-avatar Phase 1a side-by-side (2026-08-13)
refs: [viewer-hover-tooltips]
---

Context: [context/viewer.md](../context/viewer.md).

An avatar name tag that is geometrically **in front** of the clouds (the avatar
is near; the clouds are the far sky) is **occluded by** them — the far cloud
layer paints over the nearer transparent billboard. Reported on Aditi/OpenSim;
independent of the GPU-avatar work (it is the *regular* avatar name tags, and
pre-existing).

## Suspected cause (well-supported, confirm before fixing)

Two render facts collide:

- **Clouds** are a camera-centred skybox backdrop: `center_sky_on_camera`
  (`sky.rs`) keeps the dome centred on the camera every frame, and `clouds.wgsl`
  **forces the fragment depth to the far clip plane** (`sky.rs:202-203`). They
  draw in the transparent phase.
- **Name tags** render in the transparent phase with `AlphaMode::Blend`,
  **depth-tested but no depth write** (`name_tag_billboard.rs:200-204`, the
  reference's `LLGLDepthTest(GL_TRUE, GL_FALSE)`).

Because the cloud dome's origin sits at the camera, its `Transparent3d` **sort
distance is ~0** (the nearest transparent object), so it is drawn **last** —
after the name tags — despite its depth being forced far. And since name tags
do not write depth, they cannot depth-reject the later cloud draw. Net: the far
cloud layer overpaints the near tag. (Confirm by dumping the `Transparent3d`
sort keys for the cloud dome vs a name tag, or A/B by temporarily forcing the
cloud dome to sort first.)

## Fix hook

The viewer already re-sorts the transparent phase (`transparency.rs`, the same
machinery water uses — see the `sl-client-transparent-phase-resort` memory).
Force the sky/**cloud**/star domes (the depth-forced-to-far backdrops) to sort
as **backdrops** — before world-anchored transparent overlays (name tags, hover
text, parcel borders, particles) — rather than by their camera-centred origin
distance. Check the sun/moon disc too (it is placed at a real far distance,
`sky.rs:169`, so it may already sort correctly), and verify the sky dome itself
(likely opaque/early, hence only the clouds show the bug). Keep the name tags'
depth-tested/no-write behaviour (it is reference-correct for occlusion by real
world geometry).

Verify on a cloudy sky: a name tag on an avatar between the camera and a cloudy
patch stays in front of the clouds.
