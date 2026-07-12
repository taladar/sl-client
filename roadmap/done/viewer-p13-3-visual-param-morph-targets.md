---
id: viewer-p13-3
title: Visual-param morph targets
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 13 — Base avatar in the viewer (replace spheres)
---

Context: [context/viewer.md](../context/viewer.md).

**P13.3. Visual-param morph targets.** Apply
`AvatarAppearance.visual_params` (defaults where absent) → blend the base
meshes' morph-target deltas so the body takes its real shape (face, weight,
muscle, etc.). Re-morph on appearance update. One feature on top of P13.2.
**Done:** new pure `sl-avatar` module `morph` — `MorphWeights` resolves a
wire `visual_params` byte vector against the `VisualParams` table into a
`morph-target name → weight` lookup (only `param_morph`-effect params,
weighted from the appearance vector or their default; non-morph colour /
alpha / skeletal params never move geometry), built once per avatar and
reused across every base part; `MorphWeights::apply(&BaseMesh) -> MorphedMesh`
blends the part's morph-target deltas exactly as Firestorm
`LLPolyMorphTarget::apply` — `position += weight * delta` and
`normal = normalize(base + Σ weight * delta * 0.65)` (the
`NORMAL_SOFTEN_FACTOR`), producing morphed positions + normals in Second Life
Z-up space (UV / binormal deltas are silhouette-neutral and left to the base,
matching what the un-textured body needs). Driver → driven propagation stays
deferred to P13.4, so a morph param not directly transmitted sits at its
default. In `sl-client-bevy`, `to_bevy_base_mesh` is refactored onto a shared
builder and joined by `to_bevy_morphed_mesh(&BaseMesh, &MorphedMesh)` —
identical UV / skin / index data over the morphed positions / normals, so a
morphed mesh stays skin-compatible (same vertex count + `JOINT_INDEX` /
`JOINT_WEIGHT`) and a re-morph is a plain mesh swap on the same skeleton
instance. In the viewer, each rigged base-part entity now carries an
`AvatarBodyPart { agent, part }` marker, and a new `apply_avatar_morphs`
system caches each avatar's latest `visual_params` vector and, on a fresh
appearance or a just-spawned body part (`Added<AvatarBodyPart>`), rebuilds
that avatar's part meshes from the resolved `MorphWeights` — deferred and
idempotent so an appearance that arrives before the body still lands, and a
newer appearance re-morphs. Net-new library surface was three re-exports
(`MorphWeights`, `MorphedMesh`, `to_bevy_morphed_mesh`) plus the `sl-avatar`
module. Verified live on OpenSim: the agent's own `AvatarAppearance` arrives
and all 8 base parts morph (`morphed 8 body part(s) across 1 avatar(s)`) with
no skinning / wgpu errors, the rigged body re-shaping from its real
transmitted visual params.
