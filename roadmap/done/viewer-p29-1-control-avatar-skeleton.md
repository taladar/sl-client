---
id: viewer-p29-1
title: Control-avatar skeleton
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 29 — Animesh
---

Context: [context/viewer.md](../context/viewer.md).

Animated-object linksets are detected (`is_animated_object`) but rendered as
plain prims. This phase gives them their own animation-driven skeleton.

**P29.1. Control-avatar skeleton.** Give an animated-object linkset its
own `LLControlAvatar` skeleton, built from the linkset's rigged-mesh skin
joints and independent of any wearer. **Done.** A new viewer module
`animesh.rs` owns a `ControlAvatarState` resource: one *control avatar* per
animated-object root, keyed by the root's full `ObjectKey` (the id
`ObjectAnimation` names). Rather than re-deriving a skeleton from the
linkset's skin, the control avatar reuses the **standard** avatar skeleton
(the reference `LLControlAvatar` inherits the full `LLVOAvatar` skeleton, and
a rigged mesh binds to it by joint name exactly as a worn one does) via a new
`AvatarBody::spawn_bare_skeleton` — the joint-spawning half of
`AvatarState::spawn_body` with no base-body parts, attachment nodes, or name
tag. The skeleton root is an **identity child of the animesh root object
entity**, so the whole skeleton follows the object's Bevy world transform
(which already carries the Second Life → Bevy basis change + world
placement/rotation) and despawns with it — the reference viewer's
`matchVolumeTransform` pins the control avatar to the root prim's render pose
(the bind-shape rotation it also folds in is already carried by our rigged
skinning's inverse bindposes, so it is not re-applied).
`apply_rigged_attachments` now branches: an animesh linkset's rigged meshes
(detected by walking the parent chain to the animated-object root via the new
`animesh_root`, replacing the old `belongs_to_animesh` predicate) bind to the
control avatar's joints — spawned on demand at first bind via
`ControlAvatarState::ensure_spawned` — instead of a wearer's, with the wearer
agent passed as `None` (an animesh has no wearer bake, so its faces texture
from ordinary fetches, never bake-on-mesh). The rig's joint position overrides
(R1) are recorded on the control avatar rather than any wearer.
`prune_control_avatars` drops a control avatar whose root object is gone (its
entities already despawned with the object). Net-new library change was only
re-exporting `ObjectKey` / `ObjectPlayingAnimation` from `sl-client-bevy` and
adding `full_key: ObjectKey` to the viewer's `TrackedObject`. **A rigged-mesh
LOD-race fix fell out of this and is load-bearing:** an animesh is not an
attachment, so its mesh starts on the managed coarse-LOD path; the finest-LOD
upgrade (`upgrade_to_finest`) is async, but `apply_rigged_attachments` was
binding whatever `decoded()` returned *now* (the coarse 4-vertex block), and
rigged meshes are excluded from the LOD-swap rebuild — so the animesh rendered
as a collapsed few-vertex husk. `apply_rigged_attachments` now waits on
`MeshManager::lod_change_inflight(key)` before binding, so it always builds
the finest geometry. **Verified live on aditi:** the two "King Kong"
Super-Mario animesh render as correct, full-resolution rigged meshes
(previously a transparent-outline husk / single triangle).
