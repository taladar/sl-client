---
id: viewer-p34-2
title: Drive them
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 34 — Avatar cloth & body physics
refs: [viewer-p34-1, viewer-p31-12a, viewer-p17-2, viewer-p34-3]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** `LLPhysicsMotion` / `LLPhysicsMotionController` is ported. The
simulation itself is pure and lives next to the [[viewer-p34-1]] ingest, in
`sl-avatar::physics`: `BodyPhysicsState` holds one spring-damper per motion and
`step` advances all six from a `JointSample` (the joint's world position, plus
the motion's own axis rotated into world space by the joint's world rotation)
and the caller's world-up vector — so the crate needs to know nothing about
Z-up vs Y-up, and the viewer samples in Bevy world space.

Faithful to the reference, including the parts that look odd:

- the forcing term is the **joint's acceleration** along the motion axis,
  differentiated over a `30×`-stretched timestep from a `100×`-scaled world
  displacement and low-passed (`1/3` new, `2/3` old);
- the four other forces are the spring back to the user's own shape, damping on
  the *param's* velocity, quadratic drag on the *joint's* velocity, and gravity
  projected onto the axis;
- a frame is split into equal sub-steps of at most `0.05 s`, so a bounce has the
  same amplitude at 15 FPS as at 60 (`substeps` counts rather than casts, and
  reproduces the reference's `+ 1` on an exact multiple).

The result is folded into the avatar **twice**, in `sl-client-bevy-viewer`'s new
`body_physics` module, called from `pose_avatar_skeletons` (the same seam the
look-at / locomotion / reach folds use):

- the **system body** bounces through the `*_Driven` **morph params**, written
  into the [[viewer-p31-12a]] per-frame runtime-morph pipeline (so no re-bake);
- a worn **rigged-mesh body** bounces through those params' **volume morphs**,
  applied as `AnimationPose` position deltas on the `LEFT_PEC` / `RIGHT_PEC` /
  `BELLY` / `BUTT` collision-volume joints ([[viewer-p17-2]] made them
  bindable). A morph target on the system body could never reach a mesh body, so
  this is the half that matters for anyone wearing one. (The *shape* params'
  volume morphs are still unapplied — that is [[viewer-p34-3]].)

Two deliberate deviations from the reference, both documented in the code: the
first frame only **seeds** the joint trail (the reference integrates against a
zero-initialized `mPosition_world`, i.e. a region-sized jump), and a degenerate
mass makes the motion inert instead of dividing by zero.

Because `Max_Effect` defaults to **zero on every axis**, an avatar wearing no
tuned physics wearable never bounces — so the reference's own `physics_test`
switch is ported as `SL_VIEWER_PHYSICS_TEST=1` (forces every `Max_Effect` to 1),
alongside `SL_VIEWER_LOG_BODY_PHYSICS=1` (per-motion simulated positions).

Unit-tested in `sl-avatar` (sub-step split; a still joint holding the user's
shape exactly; a dropped joint bouncing and ringing back down; the
collision-volume displacement; an inactive motion driving nothing, and
`force_max_effect` switching it on; an unresolvable joint re-seeding instead of
lurching). Verified live on OpenSim with the test switch:
`6 of 6 motion(s) active`, the springs visibly tracking the avatar's motion.
