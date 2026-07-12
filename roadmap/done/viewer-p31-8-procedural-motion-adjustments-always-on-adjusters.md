---
id: viewer-p31-8
title: Procedural motion adjustments & always-on adjusters
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.8. Procedural motion adjustments & always-on adjusters.** P31.6
plays each state's *base* downloadable keyframe (walk / run / stand / turn /
fall), but not the procedural layer the reference viewer stacks on top —
which is the whole reason those states are `LLKeyframe*Motion` **subclasses**
rather than plain `LLKeyframeMotion`. Port them as post-keyframe pose
adjustments over the Phase 18 blend (they run every frame after the sampled
pose, driven by agent state, not by an `AvatarAnimation`):

- **Locomotion adjustments** — `LLKeyframeWalkMotion` matches the walk/run
  playback speed to the actual ground velocity, and the always-on
  `LLWalkAdjustMotion` foot-plants with pelvis lag, together killing
  foot-skate; `LLKeyframeStandMotion` twists the lower body / feet to face
  the look direction with foot IK (so a standing avatar's legs follow the
  camera); `LLKeyframeFallMotion` blends the landing recovery;
  `LLFlyAdjustMotion` banks the fly.
- **Always-on idle adjusters** — `LLHeadRotMotion` (head / neck tracks the
  look-at target), `LLEyeMotion` (eye saccades / tracking), `LLHandMotion`
  (hand-pose morph), `LLBodyNoiseMotion` (subtle idle sway), and
  `LLBreatheMotionRot` (chest breathing). The viewer runs these continuously
  on every avatar; they are **not** signalled over `AvatarAnimation` and so
  are absent from `sl-anim`'s table.
- **Activity-driven reach / aim** — `LLEditingMotion` reaches the hand toward
  the object the agent has selected / is editing; `LLTargetingMotion` aims the
  arm at a target (mouselook aim / the point gesture). Also locally driven,
  also absent from the table.

(`LLPhysicsMotionController` avatar body-jiggle physics is separate — Phase
34.) Reference: `llkeyframewalkmotion.cpp` / `llkeyframestandmotion.cpp` /
`llkeyframefallmotion.cpp` and the adjuster motion classes in
`indra/newview/`; the `registerMotion` block in `llvoavatar.cpp`. **Done — the
two always-on idle adjusters that need no external input landed; every other
adjuster listed above is deferred to its own item (P31.12–P31.15) because it
needs state this pass has no access to.** New `procedural.rs` (viewer-only,
like the P31.6 locomotion fallback) ports the input-free always-on pair as
pure, unit-tested functions folded into `pose_avatar_skeletons` as a
post-keyframe pass over the resolved [`AnimationPose`]: **breathe**
(`LLBreatheMotionRot`) — a slow `sin(time)·0.05` pitch of `mChest` about its
local Y, an exact port; and **body noise** (`LLBodyNoiseMotion`) — a subtle
≤1° low-frequency sway of `mTorso` about local X/Y at `TORSO_NOISE_SPEED`.
Each is composed as a small delta *on top of* whatever the keyframe pose
already animates for that joint (`base·delta`), so a playing animation still
dominates and the idle motion only shows where the joint is otherwise at rest
— the effect the reference gets from these motions' additive / low-priority
blend. Applied to every rigged avatar each frame, so an idle avatar breathes
and sways instead of standing frozen. Body noise is faithful in *character*
but not a bit-for-bit Perlin port: the reference `noise2` tables are
`llrand`-seeded every viewer startup, so there is no canonical waveform to
match, and a full port would also need a lint-scoped `as` on the one float→
lattice-index truncation (Rust std has no `From`/`TryFrom` for float→int); a
cast-free incommensurate-sine noise reproduces the slow wander for less code.
Not ported (each gates its motion by avatar pixel area — not modelled
here, the pose pass already runs only for rigged avatars): head/eye look-at
tracking, hand-pose morph, the IK locomotion adjustments, and the reach/aim
motions → **P31.12–P31.15**. Library change: `AnimationPose::{rotation,
position}` made `pub` in `sl-client-bevy` for the viewer to read-modify-write
a joint's resolved rotation. Verified: unit tests (breathe rest/peak, sway
amplitude bound, sway moves over time, delta-composes-on-keyframe-base, absent
joints skipped); build + clippy clean.
