---
id: viewer-avatar-keyframe-constraints
title: Apply an animation's own joint constraints (ground and body-volume targets)
topic: viewer
status: ready
origin: viewer-avatar-ground-from-collision-plane review (2026-11-08)
refs:
  - viewer-avatar-ground-from-collision-plane
  - viewer-p31-14-locomotion-ik-adjustments
---

Context: [context/viewer.md](../context/viewer.md).

An `.anim` can carry **joint constraints**, and the reference viewer applies
them to every keyframe motion, uploaded animations included
(`LLKeyframeMotion::applyConstraints`, `llkeyframemotion.cpp:767`–`1130`). A
constraint pins a point on a source body volume, at the end of a joint chain
`chain_length` long, to a target, easing in and out over the animation's
timeline. The target is either:

- another body volume plus an offset (a hand on a hip, two hands together);
- or the **ground** (`CONSTRAINT_TARGET_TYPE_GROUND`, the reserved target
  name `GROUND`), resolved through `getGround`, which is the same
  collision-plane-and-land ground the foot IK uses
  ([[viewer-avatar-ground-from-collision-plane]]).

The constraint type is a point or a plane (`CONSTRAINT_TYPE_PLANE`: only the
distance along the target's normal is held, e.g. a foot kept from sinking
below the ground but free to slide). The reference solves each chain
iteratively (between 2 and 20 iterations, `MIN_ITERATION_COUNT` /
`MAX_ITERATIONS`) after the keyframes are applied.

We already **decode** constraints (`sl-anim/src/decode.rs`, `Constraint`,
`ConstraintTargetType::Ground`) but drop them: no pose pass reads
`Motion::constraints`.

Why it matters: the stand foot IK (`LLKeyframeStandMotion`) only runs under
the built-in stands, so an avatar running an animation overrider never gets
it, in either viewer. An overrider animation authored with ground constraints
keeps its feet planted in the reference viewer through this path and not in
ours. Few overrider animations carry constraints, so the visible gap is
small, but it is a fidelity gap in what every uploaded animation can express.

## Scope

- Apply each playing motion's constraints in the pose pass, after keyframe
  sampling and before the locomotion adjusters, with the reference's
  activation and easing (`activateConstraint` / `deactivateConstraint`,
  ease-in and ease-out times relative to the motion).
- Ground targets through the existing per-avatar ground (`AvatarGround`);
  body-volume targets through the posed skeleton's collision volumes.
- Point and plane constraint types; the reference's chain solve and
  iteration bounds.
- Unit tests on a synthetic motion: a ground-constrained foot stays on a
  raised or sloped plane while the keyframes would lift or sink it; a
  plane constraint holds height but lets the foot slide; easing ramps the
  correction in and out.
- An e2e check where possible: the fake grid can play an uploaded
  animation carrying a ground constraint, and the `feet` probe on the agent
  readout shows the ankle's clearance over the ground.
