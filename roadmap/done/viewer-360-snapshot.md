---
id: viewer-360-snapshot
title: 360-degree (equirectangular) snapshot
topic: viewer
status: done
origin: user request (2026-07)
refs: [viewer-snapshot-floater, viewer-photo-hosting-upload, viewer-world-text-in-the-overlay-pass]
---

Context: [context/viewer.md](../context/viewer.md).

A 360-degree panorama capture — the immersive photo you can pan around or view
in a headset, which SL residents post to the platforms that render
equirectangular images. This is **not a snapshot option**; it is a distinct
capture-renderer, and Firestorm gives it its own floater (`llfloater360capture`)
for exactly that reason.

Shipped as `sl_viewer_world_view::panorama` (the capture, the floater and the
save) with two pure submodules: `panorama::equirect` (the reprojection) and
`panorama::xmp` (the metadata). Opened from **World ▸ Photo and Video ▸ 360°
Snapshot…**.

## The capture borrows the viewer's own camera

The six faces are shot by pointing the **`ViewerCamera`** at each of them in
turn, into an off-screen square image — not by standing up a second camera.
That is the same conclusion [[viewer-snapshot-floater]] reached from the other
side: the world camera carries the probe-generated image-based lighting and the
exposure / glow / underwater-fog / tone-map selectors, and a second camera that
misses one of them renders a darker, flatter world.

Borrowing it means the pose, the lens and the render target are saved and handed
back, and the two writers that would otherwise fight the borrow stand aside on a
`panorama::CameraBorrowed` resource:

- `camera::position_camera` and `camera::drive_flycam` (both write the camera
  `Transform`), and
- `sl_viewer_preferences`' `apply_camera_fov`, which otherwise rewrites the lens
  from the stored `CameraAngle` **every frame** — between a face's framing and
  its shutter.

The target is *saved*, not assumed to be the window: the resolution divisor may
already have the world camera on a reduced image. (That module leaves a camera
on a foreign target alone, so it does not fight the borrow either.)

The capture writes each face's rotation **inside**
`WorldPhase::CameraPositioned` rather than after it, so the sky dome, the ocean,
the billboards and the underwater-fog matrix — all of which order themselves
against that set — follow the face being shot rather than the previous one.

## Freezing, and waiting for the scene to arrive

A capture waits for `SceneQuiescence` (with a 20 s timeout, so a busy region
still gets its photo) and then **pauses `Time<Virtual>`** for the six faces,
which stops the water, clouds, stars, particles and animations — they all run
off the clock that feeds `globals.time`. That is the reference viewer's
`freezeWorld` through the one switch this engine already has for it.

## The reprojection, and the two places a naive stitch looks wrong

`panorama::equirect` defines the six faces in the **capture frame** — `-Z` is
where the camera was looking, levelled so a pitched camera still yields a
horizontal horizon — so the centre column of the panorama is the composed shot
and the seam falls directly behind the camera. Firestorm shoots world axes
instead and leaves the heading to the metadata, which puts its seam wherever
east happens to be.

- **Seams**: a bilinear tap near a face edge is clamped to its own face, so the
  joint is blurred by at most half a texel rather than sampling a neighbour the
  per-face image cannot see (what a GL cube map without seamless filtering does,
  which is the reference's WebGL path).
- **Poles / undersampling**: each output pixel is supersampled on a grid derived
  from the ratio between the cube's angular resolution and the output's, so
  capturing at 2048 faces for a 4096-wide panorama is averaged rather than point
  sampled.
- Every average is taken in **linear light**; averaging sRGB code values darkens
  every high-contrast edge.

Tested on synthetic cubes: face/rotation agreement, the image layout, a uniform
cube, one colour per face, a smooth analytic sphere round-tripped to within
eight code values everywhere, and the seam column pair.

## The metadata is the difference between a panorama and a wide picture

`panorama::xmp` writes the GPano block (projection, full-pano **and**
cropped-area sizes — the reference omits the latter and several readers treat
that as "not a panorama" — the pose heading, the capture dates and the
software), plus the Second Life specifics the reference writes, under a
**declared namespace** rather than as the bare, namespace-less elements it emits
(which are not well-formed RDF). `GPano:PoseHeadingDegrees` carries the compass
heading of the image centre and `InitialViewHeadingDegrees` is `0`, because our
centre *is* the composed view.

Neither encoder writes XMP, so the packet is spliced into the encoded bytes: an
`APP1` segment after the JFIF `APP0` for JPEG, an `iTXt` chunk after `IHDR` for
PNG (with a hand-written CRC-32, checked by decoding the spliced result). The
format list is JPEG and PNG only — BMP and TGA have no metadata container, so a
panorama written as one would never open as a sphere.

## Deliberately not here

- **Stereo / VR 360** (two eye points), as the task said.
- **Hide avatars / particles**, which the reference offers: that wants the
  render-type toggles ([[viewer-render-type-toggles]]) rather than a bespoke
  hide in this floater.
- The external destinations are still [[viewer-photo-hosting-upload]]'s.
- **World-space text is still in the frame.** Name tags and `llSetText` hover
  text are drawn by the world camera in the transparent phase, so they appear in
  all six faces; [[viewer-world-text-in-the-overlay-pass]] takes them out of a
  panorama for free once it lands.

Reference (Firestorm, read-only): `llfloater360capture`, its
`CubemapToEquirectangular.js` / `jpeg_encoder_basic.js`, and our own P33 probe
cube-map capture (`probes.rs`) as the nearest in-tree precedent.
