---
id: viewer-poser
title: Poser — manual joint posing (incl. pose stand)
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-animation-overrider, viewer-mesh-gltf-import]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's Poser: pose your own avatar (or a selected animesh) by rotating
individual joints in-viewer — photographers' and creators' tool. The skeleton
and animation blending are already ours (`sl-avatar`, `animations.rs`), so a
pose is "hold these joint rotations at high priority".

Scope: the joint-tree floater with per-joint rotation (and the reference's
position/scale nudges), symmetric mirroring, saving/loading poses (the
reference's XML pose format for import compatibility, plus our own), posing
selected animesh, and the simple **pose-stand** mode (lock the avatar into a
T/A-pose for fitting attachments — the reference ships it as a separate tiny
floater; here it is a preset in the same tool).

Out of scope: exporting poses as uploaded `.anim` assets (belongs with the
upload wizard if ever wanted).

Reference (Firestorm, read-only): `fsfloaterposer`, `floater_fs_poser.xml`,
`floater_fs_posestand.xml`.

Builds on: `sl-avatar` skeleton + the animation priority blender.
