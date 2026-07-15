---
id: viewer-snapshot-to-inventory
title: Save a snapshot to inventory (as a texture)
topic: viewer
status: blocked
origin: user request (2026-07)
blocked_by: [viewer-snapshot-floater, viewer-image-upload]
---

Context: [context/viewer.md](../context/viewer.md).

Save an in-world snapshot into inventory as a **texture** — the destination
that behaves unlike all the others, which is why it is split from the snapshot
floater ([[viewer-snapshot-floater]]) rather than folded into it.

What makes it different (confirmed in `llsnapshotlivepreview.cpp`): a texture is
not an arbitrary image. The capture must be constrained to **power-of-two
dimensions, biased-scaled to ≤1024** (disk output, by contrast, is free-form up
to the render limit), then **J2C-encoded** and uploaded as an asset that **costs
L$**. So the resolution picker itself changes mode by destination, and this path
carries a cost-confirmation the others do not.

That upload is the general texture path — this task feeds the image upload
wizard ([[viewer-image-upload]]) with the snapshot as its source, rather than
growing a second uploader. Scope here is the snapshot-specific part: the
power-of-two / ≤1024 constraint and its live-preview feedback, name /
description / target folder, the L$ cost confirmation, and the choice of a
permanent upload
vs. a **temporary local texture** (preview it on a prim without paying, a common
photographer workflow).

Reference (Firestorm, read-only): `panel_snapshot_inventory.xml`,
`llsnapshotlivepreview` (the `expandToPowerOfTwo` / `biasedScaleToPowerOfTwo`
constraint), `llviewerassetupload`.

Builds on: [[viewer-snapshot-floater]] (the floater and the captured image),
[[viewer-image-upload]] (the shared texture-upload path).
