---
id: viewer-render-cpu-skinning-crosscheck
title: CPU-skinning cross-check — make the R13 debug affordance a standing test
topic: viewer
status: ready
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
