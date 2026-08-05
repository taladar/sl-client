---
id: viewer-near-avatar-stuck-coarse-sphere
title: A nearby avatar stays a coarse sphere even as the camera closes in
topic: viewer
status: bugs
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

On aditi an avatar (reported at ~622 m away, on the same parcel, just above the
camera) rendered only as the coarse placeholder **sphere**, and stayed a sphere
even as the camera moved close to it — the full avatar never resolved.

Related to the R22a far/late static-T-pose routing and the R22b interest-camera
work: a coarse-only avatar placeholder that never upgrades to the full body once
it is near/interesting. The avatar was
initially far (its ObjectUpdate may have arrived only as a coarse-location dot,
or its body was requested at coarse LOD and never upgraded). Check the
placeholder→full-body promotion path (`MeshManager::upgrade_to_finest` /
`apply_rigged_attachments` and the coarse-dot reconcile) for the case where an
avatar starts far and only later comes into interest range — it should promote
to the full mesh body once close, and currently does not for this avatar.
