---
id: viewer-render-cpu-skinning-crosscheck
title: CPU-skinning cross-check — make the R13 debug affordance a standing test
topic: viewer
status: done
origin: the viewer-render-test-harness work (2026-07); the task's "cross-checks between paths", not built with the first tier
blocked_by: [viewer-render-test-harness]
refs: [viewer-render-test-harness, viewer-render-scene-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-render-test-harness]]'s "cross-checks between paths":

> The CPU-skinning reference in `sl-client-rigged-mesh-skinning` exists
> precisely to compare against the GPU result — make that a standing test rather
> than a debug affordance.

It is still a debug affordance. `avatars.rs`'s `log_geometry_outliers`
reproduces Bevy's matrix-palette skinning on the CPU
(`palette = joint_world · inverse_bind`, then `mix(M0, M1, t) · p` between the
two adjacent render-list slots), sorts vertices by displacement from the morphed
rest pose, and `info!`-logs the worst ten with the joint each weight resolves
to. It is gated at its call site by `SL_VIEWER_LOG_AVATAR_GEOMETRY`, and it is
how R13 was localised.

Two problems with it as it stands, and they are the task:

1. **The skinning maths and the reporting are fused.** It only logs. Nothing can
   *assert* on the result, so the comparison a human did by reading ten lines
   cannot be done by a machine over every vertex.
2. **`world_matrices` is threaded through `apply_avatar_appearance` solely for
   this diagnostic** (the comment says so: "kept only for the geometry
   diagnostic (R13)"). A parameter that exists for a debug print is a parameter
   that gets deleted by the next person who tidies up — taking the only R13
   detector with it.

## The work

Split a pure `fn cpu_skin_vertex(...) -> Vec3` out of `log_geometry_outliers` —
the maths, with no `info!` in it — and give it a scene in the harness's
registry. Then the check is the obvious one: for a rigged scene, every vertex's
CPU-skinned position must match what the GPU pipeline was handed, within a
tolerance. Anything that does not is R1 or R13 or their next relative.

The harness already covers the two *countable* halves of this
(`skin_violations`: weights sum to one, joints inside the render list). What it
cannot see is whether the palette itself is assembled right — bind-shape folded
in the wrong order, a transposed matrix, a joint whose world transform is stale.
Those all produce perfectly valid-looking weights and a body that bends wrong,
which is precisely the class that has cost the most here.

Depends in practice on [[viewer-render-scene-coverage]]'s real avatar scene: the
synthesized two-joint strip the harness registers today has identity binds by
design, so it cannot catch a bind-order bug. This check needs a rig where the
bind matrices are not identity.

## Outcome (2026-09-27)

- **The maths is a pure function**: `sl_client_bevy::cpu_skin_vertex(rest,
  weight, skin, joint_world) -> Option<Vec3>`, the reference's
  `mix(palette[i], palette[i + 1], fract(weight))` with the partner clamped to
  the last render-list entry, and `None` for a weight the render list cannot
  resolve. Four unit tests beside it. `log_geometry_outliers` calls it and
  keeps only the reporting.
- **The scenes declare it**: `CpuSkinnedPositions` (in
  `sl-viewer-render-fixtures`) is attached to every skinned part of
  `avatar-base-part` and `avatar-morphed-body`, computed from the vertex
  weights, the rebuilt render list and `deformed_world_matrices` — never from
  the joint entities or the Bevy mesh.
- **The harness holds the GPU to it**: `scene_geometry` gathers the palette
  exactly as Bevy's `extract_skins` builds it (joint `GlobalTransform` · inverse
  bind, a missing joint a NaN rather than a shift), and the declared-tier
  `cpu_skinning_violations` skins every vertex through the mesh's
  `JOINT_INDEX` / `JOINT_WEIGHT` attributes and that palette, within 0.1 mm of
  the reference. It runs in every cell of the scene × LOD × sample sweep.
  `every_skinned_avatar_part_declares_its_cpu_skin` keeps it from going
  vacuous.
- **It bites where it has to.** Swapping the two blend weights in
  `build_base_mesh` — valid weights, wrong palette entries — is reported on
  five parts of the shaped body, up to 15 cm off; the rest body stays green,
  since at the bind pose every palette entry is the identity. That asymmetry
  is the file's own prediction: the check needs non-identity binds, and
  `avatar-morphed-body` supplies them.
- **`world_matrices` is no longer the only detector.** It is still threaded
  through `apply_avatar_appearance` for the log, which names an outlier vertex
  on a live body; deleting it now loses a diagnostic, not the check.

Found nothing wrong in the current pipeline. The one suspicion going in —
`build_base_mesh` clamps the blend partner to the part's largest weight index
where the reference clamps to the render list — is harmless on the Linden body
as shipped: the shaped body agrees vertex for vertex, so no vertex at that
index blends onto a partner the two clamps would resolve differently, and if one
ever does, this check reports it.
