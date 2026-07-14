---
id: viewer-p31-14
title: Locomotion IK adjustments
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.14. Locomotion IK adjustments.** The locomotion group split out of
P31.8 — every piece needs an **inverse-kinematics solver** the project does
not have. `LLKeyframeWalkMotion` matches the walk/run playback speed to the
ground velocity, and the always-on `LLWalkAdjustMotion` foot-plants with
pelvis lag to kill foot-skate; `LLKeyframeStandMotion` twists the lower body /
feet to face the look direction with foot IK (a standing avatar's legs follow
the camera); `LLKeyframeFallMotion` blends the landing recovery;
`LLFlyAdjustMotion` banks the fly. Build a small foot / limb IK pass first,
then layer these over the P18 blend. Reference: `llkeyframewalkmotion.cpp` /
`llkeyframestandmotion.cpp` / `llkeyframefallmotion.cpp`.

## Done

All five adjusters ported, plus the two pieces of infrastructure they needed.

- **`ik.rs`** — `LLJointSolverRP3`, the reference's closed-form two-bone solver
  (pole vector, knee hinge axis, twist). Pure and unit-tested; P31.15's
  reach/aim adjusters reuse it as-is.
- **`ground.rs`** — the `LLVOAvatar::getGround` equivalent: a short vertical
  `MeshRayCast` against the rendered world under the body root and each ankle.
  Object-aware, so the feet plant on a prim ramp or platform, not just terrain
  (a terrain lookup cannot see a ramp; nor can a physics query, since a static
  walkable prim carries no avian collider — only *physical* ones do).
- **`locomotion_ik.rs`** — the `LLWalkAdjustMotion` foot-slip speed servo,
  `LLKeyframeStandMotion`'s foot IK + ankle-to-surface roll,
  `LLKeyframeFallMotion`'s landing recovery, and `LLFlyAdjustMotion`'s bank.
- **`animations.rs`** — `LLKeyframeWalkMotion`'s speed-scaled playback clock, as
  a `PlayState::anim_offset` so the animesh path and every non-walk motion keep
  wall time untouched.
- **`sl-anim`** — `keyframe_motion_class()` / `is_walk_adjust_trigger()`, ported
  from the reference's `registerMotion` block and `AGENT_WALK_ANIMS`.

**Two reference behaviours are vestigial and were ported as they *behave*, not
as they read:** `LLWalkAdjustMotion::mPelvisOffset` (the "pelvis lag") is
commented out upstream with a `FIXME` saying it fights the speed servo, and
`LLKeyframeStandMotion::mTrackAnkles` is set `true` in three places and `false`
in none, so its ankle-*locking* is dead. Details in the module docs.

**One deliberate deviation:** the foot IK aims each ankle at its keyframe height
displaced by the ground's rise/fall under that foot, not at the ground's
absolute height. The reference's skeleton root sits at the *pelvis*; ours sits
at the *sole*, so the absolute form would drive our ankle 6.7 cm too low and
bury the foot. Same correction, different frame.

**Live-verified on OpenSim** (17° terrain hill + a 20° prim ramp provisioned for
the purpose): walk servo settles at 1.16–1.26× forward and goes negative
walking backwards; foot IK steady and correct on both the hill and the *prim*
ramp; a bit-for-bit no-op on flat ground. Two bugs were found only in-world and
are documented at length in `locomotion_ik.rs` — a probe/IK feedback loop that
buzzed the knees on slopes, and a reach clamp that lifted the avatar onto its
toes when it stopped. Both stem from a standing leg being a near-singular IK
chain (~99.5% extension, unbounded gain).

**Not seen move:** `LLKeyframeFallMotion`. It hangs off `standup`, which only
the simulator sends on landing from a real fall; the client-side locomotion
fallback plays `falldown`, never `standup`. Ported and compiling, but never
observed running.
