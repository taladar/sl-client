---
id: viewer-p17-1
title: Skinning math
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 17 — Rigged mesh & bake-on-mesh
---

Context: [context/viewer.md](../context/viewer.md).

**P17.1. Skinning math.** In `sl-avatar` `skin.rs`: a matrix-palette
helper taking `sl_mesh::MeshSkin` (joint names + inverse-bind + bind-shape +
alt-bind + `pelvis_offset` + `lock_scale_if_joint_position`) and per-vertex
`VertexWeights` against a `Skeleton` instance's current joint world transforms
→ skinned vertices (≤4 weights). Tests with a synthetic skeleton.
**Shape:** `SkinningPalette::build(&skin, |name| Option<world_matrix>)` folds
each rig joint into `inverse_bind_matrix[j] * joint_world_matrix[j]`;
`skin_position` / `skin_normal` then apply `v * bind_shape` and the
weight-normalized blend of the palette matrices (mirroring Firestorm's
`initSkinningMatrixPalette` + `getPerVertexSkinMatrix` +
`updateRiggedVolume`). All matrices are SL's row-vector row-major `[f32; 16]`
(same layout `sl-mesh` decodes), so this stays Bevy-free and glam-free — a
hand-rolled `[f32; 16]` mat-mul / affine transform under the crate's strict
lints. The joint world transforms are an **input**: the caller (P17.2) poses
the skeleton instance, and `alt_inverse_bind` / `pelvis_offset` /
`lock_scale_if_joint_position` are honoured there (they shape the world
matrices), not in the palette algebra. Missing-joint fallback matches the
reference viewer (world = identity → palette entry is the bare inverse-bind).
10 unit tests over a synthetic skeleton (translation/blend/normalization,
inverse-bind↔world cancellation, bind-shape ordering, missing/out-of-range
influences, normal rotation without translation). New `sl-avatar → sl-mesh`
dependency for `MeshSkin` / `VertexWeights`.
