---
id: viewer-r1
title: Rigged-mesh skinning distortion
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

Avatar / prim rendering-fidelity bugs, several surfaced by the live BoM-avatar
review on aditi in P17.3. These are **pre-existing** gaps separate from the
feature phases above; each needs iterative visual debugging against a live
avatar, so they are collected here to be worked one at a time.

**R1. Rigged-mesh skinning distortion.** Two independent fixes, the first
being the actual cause of the visible distortion (confirmed live: pants, feet,
and the mesh-head teeth / eyes / eyelids all render cleanly after it).

- **Un-normalized skin weights (the real fix).** A worn rigged mesh's
  per-vertex weights were fed to Bevy raw. Second Life stores each influence
  as an independent quantized fraction and drops influences past the fourth,
  so a vertex's weights need not sum to one — but Bevy's skinning shader
  (unlike the reference viewer's `getPerVertexSkinMatrix`) does **not**
  renormalize, so a weight sum `s < 1` blends in `(1 - s)` of the zero matrix
  and drags the vertex a fraction of the way to the mesh origin — the downward
  "streak toward the feet" of a rigged garment / head part. Fixed by
  renormalizing the four weights to sum to one in `pack_influences`
  (`sl-client-bevy` `meshes.rs`); a zero-sum vertex binds fully to slot 0.
  This is what fixed the pants / feet and, as a bonus, the BoM-head teeth /
  eyes / eyelids (also worn rigged mesh). The base system body was never
  affected — it uses the (already normalized) adjacent-joint blend path.
- **Joint position overrides (fitted-body proportions / fingers).** A fitted
  mesh body/head also ships an `alt_inverse_bind_matrix` per joint (the upload
  "include joint positions" option) that repositions the skeleton to the pose
  its inverse-binds assume; a worn rigged mesh carries its **own**
  inverse-binds, so without the overrides its extremities sit slightly off
  (the base body self-cancels, being skinned against *our own* bindposes).
  Implemented as the reference viewer's `addAttachmentOverridesForObject`:
  `joint_position_overrides` / `JointOverrides` +
  `BevySkeleton::deformed_local_transforms_with` (0.1 mm threshold, replaces
  the joint's local rest position, honours `lock_scale_if_joint_position`),
  stored per contributing mesh so a per-joint conflict resolves to the highest
  mesh id (`findActiveOverride`) and the set rebuilds as meshes come and go
  (`clearAttachmentOverrides`). **Animesh (animated objects) are excluded** —
  they drive their own control-avatar skeleton (`!vo->isAnimatedObject()`),
  detected via the linkset root's `ExtendedMesh` `ANIMATED_MESH_ENABLED` flag;
  without this a giant / rotated-frame animesh worn nearby would catapult the
  wearer's skeleton. On the test avatar its own body's overrides are ≈0, so
  this part is a near-no-op there; it targets bodies that genuinely reposition
  joints. Toggle `SL_VIEWER_JOINT_OVERRIDES=0` disables it. `pelvis_offset` is
  left unapplied (a hover/height concern, not distortion; `0.0` on every
  observed body).
