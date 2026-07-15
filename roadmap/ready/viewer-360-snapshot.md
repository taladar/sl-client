---
id: viewer-360-snapshot
title: 360-degree (equirectangular) snapshot
topic: viewer
status: ready
origin: user request (2026-07)
refs: [viewer-snapshot-floater]
---

Context: [context/viewer.md](../context/viewer.md).

A 360-degree panorama capture — the immersive photo you can pan around or view
in a headset, which SL residents post to the platforms that render
equirectangular images. This is **not a snapshot option**; it is a distinct
capture-renderer, and Firestorm gives it its own floater (`llfloater360capture`)
for exactly that reason.

The mechanism: render the scene into the **six faces of a cube map** from the
camera position, then **reproject to an equirectangular** panorama (the 2:1
lat/long image the format wants). We already render cube-map faces for the P33
reflection probes, so the capture path has a close cousin in the codebase to
build on — but at photo resolution, not probe resolution.

Scope and the parts that make it more than "screenshot ×6":

- Capture all six faces at a chosen resolution from one eye point, with the
  environment (sky, water, sun) consistent across faces — and reuse the
  snapshot floater's quiescence wait so avatars, mesh and textures have finished
  streaming before the shutter (see [[viewer-screenshot-wait-for-quiescence]]).
- Equirectangular reprojection, with attention to the **seams** between faces
  and the **nadir/zenith** poles where a cube map is worst — this is where a
  naive six-shots-and-stitch looks wrong.
- Write the standard **XMP / GPano metadata** so viewers and platforms recognise
  the image as a 360 panorama.
- Output at the destinations the snapshot floater already offers
  ([[viewer-snapshot-floater]]) and the external ones
  ([[viewer-photo-hosting-upload]]).

Deliberately out of scope: stereo/VR 360 (two eye points) — note it as a
possible follow-up, do not build it here.

Reference (Firestorm, read-only): `llfloater360capture`, and our own P33 probe
cube-map capture (`probes.rs`) as the nearest in-tree precedent.

Deps: [[viewer-snapshot-floater]] (the floater, format selection and
destinations).
