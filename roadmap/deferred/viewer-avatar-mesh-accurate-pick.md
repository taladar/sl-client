---
id: viewer-avatar-mesh-accurate-pick
title: Mesh-accurate avatar picking (replace the bounding-box approximation)
topic: viewer
status: deferred
origin: viewer-avatar-context-menu review (2026-07)
blocked_by: [viewer-avatar-context-menu]
refs: [viewer-avatar-radar, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-avatar-context-menu]] picks an avatar with an invisible
**box collider** (`fit_avatar_pick_colliders` in `src/avatars.rs`), because a
`MeshRayCast` tests a skinned mesh against its **bind** pose (a T-pose at the
origin), not the posed vertices, so it cannot hit the avatar where it is drawn.
The box is sized from the **posed skeleton** each frame — height from the
joints' vertical span (shape- and pose-adaptive), width/depth from the
reference's fixed `DEFAULT_AGENT_WIDTH` / `DEFAULT_AGENT_DEPTH` — so it hugs the
torso and tracks the avatar. **That shape/pose-fitting is done.**

What is **not** done, and is this task: the box is not silhouette-accurate. A
click just off an avatar, or between two overlapping avatars, can pick it — the
box is bigger than the avatar's pixels. The reference does **not** have this
problem: its `mBodySize` box is only the *physics* volume, and its avatar
*selection* uses `LLVOAvatar::lineSegmentIntersect` against the
**rendered mesh** — pixel-accurate. (Our nearest-hit tie-break already means the
*closer* of two overlapping avatars wins, which softens but does not remove the
imprecision.)

Make our pick mesh-accurate, matching the reference. The obstacle is the posed
geometry; the options:

1. **CPU-skin a pick copy.** Keep a CPU-side posed-vertex buffer per avatar
   (apply the same joint palette the GPU uses) and ray-test the triangles — what
   the reference effectively does. Exact; costs a skinning pass per avatar per
   pick (or per frame if cached). CPU-skin scaffolding already exists for the
   rigged-mesh debug repro (see the `sl-client-rigged-mesh-skinning` memory).
2. **Read back / snapshot posed vertices.** Snapshot the skinned positions (a
   compute pass or transform-feedback equivalent) into a mesh the ray caster can
   hit. Exact but heavier plumbing.
3. **Per-bone capsules.** Approximate the body with a capsule per major bone
   (upper arm, forearm, thigh, shin, torso, head), fitted to the joints. Much
   closer to the silhouette than one box, no skinning — a middle ground.

Prefer (1) if the CPU-skin path is cheap enough to run on demand (only on a
right-click / drag-hover, not every frame). Keep the box as the fallback for
avatars whose mesh has not decoded yet. The pick entry point is
`request_avatar_menu_on_right_click` (`src/avatar_menu.rs`); the collider and
its fit live in `src/avatars.rs`. This also matters for
**inventory drag-and-drop onto an avatar**, which will reuse the same pick.
