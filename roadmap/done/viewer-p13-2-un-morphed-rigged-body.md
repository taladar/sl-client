---
id: viewer-p13-2
title: Un-morphed rigged body
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 13 — Base avatar in the viewer (replace spheres)
---

Context: [context/viewer.md](../context/viewer.md).

**P13.2. Un-morphed rigged body.** `--viewer-assets <dir>` flag; load
the `character/` assets once into an `AvatarAssetLibrary` resource (skeleton +
base meshes + params), reading files here (crate stays I/O-free). In
`avatars.rs`, for each `pcode == 47` object spawn the rigged base body (all
parts) skinned to a fresh skeleton instance in the **default (un-morphed) rest
shape**, replacing the placeholder sphere; keep the sphere as fallback when no
assets / load fails, and keep the name tags. Verify a body renders on OpenSim.
**Done:** new viewer module `avatar_assets.rs` owns the disk read — the
`--viewer-assets <dir>` flag (env `SL_VIEWER_ASSETS`) points at an installed
Firestorm / Second Life `character/` directory, and
`AvatarAssetLibrary::load` (via `fs_err`, the workspace-sanctioned reader)
parses `avatar_skeleton.xml` → `BevySkeleton`, `avatar_lad.xml` →
`VisualParams` (kept for the P13.3 / P13.4 morph phases), and the eight
`lod = 0` base-part `.llm` files named by the `avatar_lad.xml` `<mesh>`
table (head, hair, eyelashes, upper body, lower body, skirt, and the two
eyeballs). Each part's skeleton binding is resolved at load and a part whose
binding is unresolvable is skipped (logged), not fatal: the six weighted
parts resolve their own joint-name table against the skeleton into a
`BaseMeshSkin` (`Skinned`), while `avatar_eye.llm` carries **no** skin
weights and no joint table, so each eyeball is bound `Rigid` to a single eye
joint (`mEyeLeft` / `mEyeRight`) and simply follows it. A load failure or an
absent flag logs and leaves avatars as Phase-10 spheres. A Startup system
(`setup_avatar_body`) builds the per-avatar-**invariant** render assets once
into an `AvatarBody` resource — one shared Bevy `Mesh` per part (via the
P13.1 `to_bevy_base_mesh`), one shared `SkinnedMeshInverseBindposes` per
skinned part, one shared skin `StandardMaterial`, and the joint rest
transforms / parent indices a fresh skeleton instance is spawned from. In
`avatars.rs`, `apply_object` now spawns, per full-object avatar, a body-root
anchor entity carrying the single Second Life → Bevy basis change, a fresh
joint-entity hierarchy under it, a `SkinnedMesh` per skinned part (its
`joints` mapped from the part's `JOINT_INDEX` table to this instance's joint
entities) parented to the root, and each rigid eyeball parented to its eye
joint entity. Because Bevy skinning derives each vertex's world position
solely from the joint `GlobalTransform`s (`world_from_local =
skin_model(...)`, ignoring the mesh entity's own transform), the axis change
carried by the root joints lands the Second-Life-space geometry correctly in
Bevy's Y-up world with no per-mesh transform. The root is lowered by the
pelvis rest height so the pelvis sits at the reported object position (Second
Life reports an avatar near its pelvis); moving an avatar re-applies that
transform, and the name tag now floats at a fixed head height over a
generalized `AvatarAnchor` (sphere or body root) rather than the old
sphere-only marker. Coarse-only (minimap) avatars stay spheres — only full
objects get bodies. Net-new library change was only three `sl-avatar`
error-type re-exports from `sl-client-bevy` (`SkeletonError` / `ParamError` /
`BaseMeshError`) for the loader's error enum; `cargo test -p
sl-client-bevy-viewer` gains a `body_root_transform` planting test (24 total
green). Verified live on OpenSim (Default Region, user-confirmed on screen):
an **untextured default "Ruth" avatar in the T-pose** rest shape replaces the
placeholder sphere — no skinning / wgpu validation errors, the skinned body
rendering in bind pose exactly as authored.
