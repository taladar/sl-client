---
id: viewer-r6
title: Avatar disappears when the camera zooms in close
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R6. Avatar disappears when the camera zooms in close.** A Bevy skinned
mesh's frustum bounds are its static bind-pose AABB placed at the mesh
*entity's* transform, while the vertices render wherever the joint matrices
put them — so the bounds need not match the drawn mesh even at rest, and the
narrow near frustum of a close camera misses them, culling the avatar. Fixed
with `NoFrustumCulling` on the avatar body parts and worn rigged meshes (so a
close camera passes through the body as in Second Life). The near plane is
unrelated (it can only clip front faces, not vanish the whole avatar; and a
perspective near plane cannot be `0`).
