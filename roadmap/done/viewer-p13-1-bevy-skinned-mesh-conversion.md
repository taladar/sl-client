---
id: viewer-p13-1
title: Bevy skinned-mesh conversion
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 13 — Base avatar in the viewer (replace spheres)
---

Context: [context/viewer.md](../context/viewer.md).

**P13.1. Bevy skinned-mesh conversion.** In `sl-client-bevy`: build a
per-avatar Bevy skeleton instance (joint entity hierarchy + `SkinnedMesh`
inverse bindposes) from `sl_avatar::Skeleton`, and `to_bevy` for each base-
body part → a `Mesh` with `JOINT_INDEX` / `JOINT_WEIGHT` attributes. Add the
`sl-avatar` dep + re-exports (`Skeleton`, `BaseMesh`, `VisualParams`,
`AvatarAppearance`). Mirror P4. **Done:** new `avatars.rs` module, the
system-avatar counterpart of `meshes.rs` / `prims.rs`.
`to_bevy_base_mesh(&BaseMesh) -> Mesh` builds a `TriangleList` with
position / normal / UV0 and, when the part is weighted, `JOINT_INDEX`
(`Uint16x4`, named explicitly since `Vec<[u16; 4]>` has no unambiguous
`Into<VertexAttributeValues>`) + `JOINT_WEIGHT` (`Float32x4`): the legacy base
body binds each vertex between two *adjacent* joints in the part's own
joint-name table, so only the first two of Bevy's four influence slots are
used (`[joint, joint+1 clamped, 0, 0]` / `[1-blend, blend, 0, 0]`) and the
joint indices are the part-local table order. `BevySkeleton::from_skeleton`
converts the parsed skeleton into per-joint local rest `Transform`s, parent
indices, and rest global (bind-pose) matrices — the data a joint-entity
hierarchy is spawned from (the actual `commands.spawn` stays in the viewer at
P13.2, so this module holds no `World` / `Commands`, mirroring how P4 returns
a `Mesh` and P5 spawns). Rest rotations are the file's Euler XYZ **degrees**;
`euler_deg_to_quat` reproduces Firestorm `mayaQ(x, y, z, XYZ)` (apply X, then
Y, then Z), which in glam's column-vector convention is
`qz.mul_quat(qy).mul_quat(qx)`. Transforms/geometry stay in Second Life Z-up
space (the viewer applies the axis change once at the avatar root, as terrain
and object meshes do). `BevySkeleton::base_mesh_skin(&BaseMesh)` resolves a
part's joint-name table against the skeleton into a `BaseMeshSkin`
(skeleton joint indices + parallel inverse bindposes) the viewer feeds into a
`SkinnedMesh`, returning `None` if any joint name is absent.
`cargo test -p sl-client-bevy` (6 new unit tests, reusing `sl-avatar`'s
committed `mini_skeleton.xml` / `mini_basemesh.llm` fixtures via
`include_str!` / `include_bytes!`): joint/root/parent + alias round-trip,
bind-pose translation composing down the hierarchy, a 90°-yaw Euler check,
one-per-vertex skin attributes with the two-slot partition-of-unity weights,
cross-asset skin resolution, and the missing-joint `None`.
