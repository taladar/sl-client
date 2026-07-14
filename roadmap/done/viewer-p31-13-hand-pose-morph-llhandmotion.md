---
id: viewer-p31-13
title: Hand-pose morph (LLHandMotion)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
blocked_by: [viewer-p31-12a]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** Ported `LLHandMotion` in full, as a new `crate::hand_pose` module
driven through the per-frame runtime-morph pipeline ([[viewer-p31-12a]]) the
blink already uses — no keyframe track is involved, because Second Life does not
pose the finger joints from an animation: it **morphs** the hands.

- **Which pose is requested.** `AnimationPlayback::requested_hand_pose` resolves
  the reference's `applyKeyframes` arbitration: every active motion (including
  one easing out) offers the `hand_pose` its `.anim` header carries (P18.1), and
  the one with the highest `Motion::max_priority` wins. `max_priority` is new in
  `sl-anim` — `LLJointMotionList::mMaxPriority`, the highest *explicit* joint
  priority, which the reference uses for this and nothing else. A tie is won by
  the **oldest** activation, faithful to the `>=` in the reference's guard
  against its newest-first active list (the opposite of the per-joint pose
  blend's tie-break; both fall out of that one list order).

- **How the morph is driven.** The thirteen `Hands_*` `avatar_lad.xml` morphs
  join `RUNTIME_MORPH_PARAMS`, so they are excluded from the static appearance
  bake and built as GPU morph targets on the upper body instead.
  `drive_hand_poses` cross-fades the outgoing pose's weight down and the
  incoming one's up over `HAND_MORPH_BLEND_TIME` and writes every pose's weight
  into `AvatarRuntimeMorphs` each frame.

- **The spread pose (index 0) has no morph** — it is the base mesh's own hand
  shape — so fading to or from it only moves the *other* pose's weight, and the
  resting default is `HAND_POSE_RELAXED`, not spread. Consequence: avatars now
  rest with relaxed hands rather than the base mesh's splayed fingers.

Unit-tested (cross-fade, the relaxed fallback, the spread special case, the
reference's guard against re-requesting a pose still fading away, weights stay
normalised across a pose sequence). **Live-verified against OpenSim**: with
`SL_VIEWER_HAND_POSE_TEST=3` the fingers visibly curl (the system-avatar fist
morph is geometrically weak, so it reads as "curled" rather than a tight fist —
the same in the reference viewer), and with no forced pose the `T` typing toggle
(P31.9) drives the hands to `Hands_Typing` and back to `Hands_Relaxed`, logged
via `SL_VIEWER_LOG_HAND_POSE=1`.

**P31.13. Hand-pose morph (`LLHandMotion`).** The always-on adjuster split
out of P31.8 that morphs the hand between the named hand-pose animations (the
`HandPose` a `.anim` header carries, P18.1), cross-fading when the pose
changes. Needs the hand-pose animation assets and a morph/blend between two
keyframe hand poses. Reference: `LLHandMotion` in `indra/newview/`.
