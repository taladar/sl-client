---
id: viewer-p18-3
title: Drive the skeleton
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 18 — Animations (full pipeline)
---

Context: [context/viewer.md](../context/viewer.md).

**P18.3. Drive the skeleton.** On `Event::AvatarAnimation`, for each
`PlayingAnimation` sample its `Motion` each frame and pose the target avatar's
skeleton-instance joints (via a `sl-client-bevy` animation driver / Bevy
clip). Attachments (Phase 16) and rigged mesh (Phase 17) follow automatically.
Verify a walking/waving avatar. **Done.** Pure sampling lives in a new
`sl-anim` `sample` module (inherent `Motion` / `JointMotion` methods,
Bevy-free): `Motion::playback_time` maps elapsed seconds to the time within
the motion honouring loop points (mirrors `LLKeyframeMotion::onUpdate`),
`is_expired` retires a finished one-shot, and
`JointMotion::sample_rotation` / `sample_position` interpolate the keyframe
curves (the reference viewer's `RotationCurve` / `PositionCurve` `getValue` +
`nlerp`, so `.anim` rotations widen to unit quaternions). `sl-client-bevy`
gains a `sample_motion(&Motion, elapsed) -> Vec<SampledJoint>` adapter (SL
Z-up `Quat` / `Vec3`, the animation mirror of `to_bevy_*`). The viewer's
`animations.rs` grew the driver: `drive_avatar_skeletons` (Update) folds each
`AvatarAnimation` set into a playback clock (a fresh `sequence_id` restarts a
motion) and resolves a per-joint `AnimationPose` (highest joint priority wins
across concurrent motions — full ease / blend is P18.4), and
`pose_avatar_skeletons` (PostUpdate, after transform propagation) writes each
rigged avatar's joint **world matrices** straight into their
`GlobalTransform`s. Verified live on OpenSim: the agent's own avatar plays a
built-in `.anim` (a new `--play-animation <uuid>` debug flag drives the own
avatar via `Command::PlayAnimation`, added on user request to exercise the
driver from a single login), fetched over `ViewerAsset` from OpenSim's
library asset set, decoded off-thread (dance1 = 19 joint tracks / clap = 10),
and the skeleton posed and returned to rest. Three fixes fell out of live
testing, all in the render crates: (1) the driver writes joint globals
**directly** rather than overlaying the keyframe rotation onto the
baked-scale rest `Transform` (a local `T·R·S` shears a non-uniformly-scaled
joint under rotation) — `BevySkeleton` gained `deformed_world_matrices(deform,
overrides, pose)`, the SL skeletal recurrence with the animation pose folded
in, and an `AnimationPose` type; (2) a position track (`mPelvis`) is a
**relative** offset *added* to the rest position, not an absolute one that
would collapse the pelvis ~1 m to its parent origin; (3) every rigged
avatar's globals are rewritten **each frame** (its animated pose or its plain
deformed rest) so an avatar un-freezes to rest when its motions stop and
several overlapping motions with different runtimes compose — Bevy's
dirty-bit propagation cannot recompute a static joint whose global the driver
overwrote. **The limb distortion this originally noted (R11) is now fixed** —
it was never the `LLSkinJoint` pivot scheme (a proven sub-millimetre no-op)
but the R13 base-mesh render-list bug (extended-ancestor weight shift); with
R13 in place the base body skins cleanly under animation (R11 verified).
