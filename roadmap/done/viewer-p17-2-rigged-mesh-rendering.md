---
id: viewer-p17-2
title: Rigged-mesh rendering
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 17 — Rigged mesh & bake-on-mesh
---

Context: [context/viewer.md](../context/viewer.md).

**P17.2. Rigged-mesh rendering.** A mesh object with a skin block worn on
an avatar renders as a Bevy `SkinnedMesh` bound to that avatar's skeleton
instance (not a static child), so mesh bodies/clothing deform with the avatar.
Reuse the `MeshManager` fetch/decode; join to the avatar via the Phase-16
attachment association. **Shape:** `MeshManager` now decodes the skin block
alongside geometry; `apply_object_meshes` diverts a *worn* rigged mesh
(attachment + skin) to a deferred `PendingGeometry::RiggedMesh`, and a new
`apply_rigged_attachments` system binds it once the wearer's skeleton instance
exists — spawning one `SkinnedMesh` submesh under the avatar body root, joints
resolved from the skin's `joint_names` (unknown → pelvis fallback, logged).
`to_bevy_rigged_mesh` / `rigged_inverse_bindposes` (in `sl-client-bevy`) build
the `JOINT_INDEX`/`JOINT_WEIGHT` attributes and fold the bind-shape into each
inverse bindpose (row-major `[f32;16]` → `Mat4::from_cols_array` is the needed
transpose). **Crucial live finding:** mesh bodies/clothing rig heavily to the
avatar's **collision volumes** (`PELVIS`, `BELLY`, `L_UPPER_ARM`, …), not just
bones — so `BevySkeleton::from_skeleton` now appends each bone's collision
volumes as extra joints (parented to their bone at the `avatar_skeleton.xml`
pos/rot/**scale**, matching the reference viewer's `setupBone`); without them
every collision-volume weight fell back to the pelvis and the mesh ballooned
into a sphere. Verified live on aditi (a worn mesh body + clothing binds and
deforms correctly; the body's own **skin** stays untextured until P17.3).
