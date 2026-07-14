---
id: viewer-p31-15
title: Activity-driven reach / aim (LLEditingMotion / LLTargetingMotion)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.15. Activity-driven reach / aim (`LLEditingMotion` /
`LLTargetingMotion`).** The activity group split out of P31.8, needing
selection / target state the viewer does not track. `LLEditingMotion` reaches
the hand toward the object the agent has selected / is editing;
`LLTargetingMotion` aims the arm at a target (mouselook aim / the point
gesture). Both are locally driven and want a limb IK reach (shares the P31.14
solver). Reference: the two motion classes in `indra/newview/`.

## Done

Both motions ported into a new `reach.rs`, plus the target state they run on.
The P31.14 IK solver (`ik.rs`) was reused **unchanged**, as that task predicted.

- **`LLEditingMotion`** — a two-bone IK solve on `mShoulderLeft → mElbowLeft →
  mWristLeft` toward the avatar's point-at target, with the reference's
  `mWristOffset` end effector (the *hand*, not the wrist joint), its target lag,
  its `HAND_POSE_RELAXED_R` hand-pose request (through the P31.13 morph
  pipeline), and its edit-plane fold.
- **`LLTargetingMotion`** — the additive torso twist that turns the avatar's
  right hand onto its look-at direction, constrained to `π/2 · 0.8` on the
  *total* (keyframe + twist) torso rotation. Gated on the reference's
  `AGENT_GUN_AIM_ANIMS`, added to `sl-anim` as `is_gun_aim_trigger()`.

**The state the viewer did not track, now tracked.** Neither motion is driven by
the animation stream; the reference drives both from HUD-effect targets, so both
halves of the point-at effect are new here:

- an own-avatar **selection** (`E` picks the object under the crosshair, `E` on
  nothing clears it), published to the simulator as a `ViewerEffect` so other
  viewers see the reach, and re-resolved against the selected object's *live*
  transform each frame so the arm follows a moving target;
- the **receive** path, which starts the editing motion on any avatar whose
  point-at effect arrives — including resolving the wire position correctly,
  which is an *offset in the target object's frame* when the effect names an
  object and a global position only when it does not (`ObjectState::entity_of`
  is new for this).

The aim target needed nothing new: P31.12 already tracks a look-at point for
every avatar (the own avatar's is the debug camera, its documented mouselook
stand-in — so an aiming own avatar tracks the camera).

**One reference oddity ported as it behaves, not as it reads.**
`LLEditingMotion` folds a target lying behind its "edit plane" with
`target + normal * (dot * 2)` — which, `dot` being negative, pushes the
direction *further* off the plane rather than mirroring it across (a reflection
would subtract). What actually makes such a target reachable is the up-to-5 m
lift applied alongside it: the arm goes **up and over** rather than around. Kept
faithful, and the arithmetic is spelled out in `edit_goal`'s docs so the next
reader does not "fix" it.

Its `mTorsoState` is a second such case: added to the motion's pose with `ROT`
usage and never assigned, so an identity rotation blends in at `HIGH_PRIORITY`
and the avatar *straightens up* as it reaches. Ported.

**Live-verified on OpenSim.** Both motions confirmed in-world. The two
self-checks the diagnostic (`SL_VIEWER_LOG_REACH=1`) reports:

- the targeting twist turned the torso 66° and left the right hand aiming
  **0.8° off** its target — i.e. the aim genuinely converges, rather than merely
  turning the torso by some amount;
- the editing solve points the arm to within **3.1°** of its goal (the rest is
  the target lag, not solve error). That is the number that matters: a
  *distance* to the goal is not, since any object more than an arm's length away
  (most of them) leaves the hand metres short **by design** — the reference
  straightens the arm and points. Misreading that cost a debug cycle; the
  diagnostic now reports the pointing angle instead.

**One bug found only in-world:** the selection raycast queried `With<Camera3d>`
and silently matched nothing, because the P33 reflection probes spawn cameras of
their own — so `E` did nothing at all, with no log line to say why. It queries
`With<FlyCamera>` now (as the look-at does) and warns when it finds no camera.
Any future system that casts a ray from "the camera" must do the same.
