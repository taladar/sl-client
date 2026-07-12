---
id: viewer-p18-4
title: Priority blending
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 18 — Animations (full pipeline)
---

Context: [context/viewer.md](../context/viewer.md).

**P18.4. Priority blending.** Resolve concurrently-playing animations
per-joint by priority with ease-in/out transitions (higher priority wins a
joint, blend on start/stop). Verify layered animations (e.g. an AO stand + a
gesture) compose correctly. **Done.** Two new pure pieces in `sl-anim`
(Bevy-free, unit-tested), mirroring the reference viewer: (1) the ease
weighting — `Motion::pose_weight(elapsed, stopped_at)` reproduces
`LLMotionController::updateMotionsByType`'s per-frame `setWeight` (cubic
ease-in from activation, hold, cubic ease-out around the stop, the residual
the ease-out scales captured at the stop so a stop mid-ease-in fades from the
partial weight), a non-looping motion auto-easing-out to finish at its
`duration` (the reference's `mSendStopTimestamp`), plus `is_finished` and a
private `cubic_step`; (2) a `blend` module — `blend_joint(&mut
[JointContribution]) -> BlendedJoint`, the pure counterpart of
`LLJointStateBlender::blendJointStates`: order the per-joint contributions by
priority (recency breaking ties), cap to the reference's four slots
(`MAX_JOINT_CONTRIBUTIONS`), then fold each channel highest-priority-first
(`new_sum = min(1, weight + sum)`, `nlerp`/`lerp` the accumulated toward the
incoming by `sum / new_sum`) so a higher-priority motion dominates a joint
while a lower-priority one shows through the unfilled weight, skipping
zero-weight (fully-eased-out) contributions. `.anim` keyframe motions are
always normal-blend, so the additive path is not modelled. The viewer's
`animations.rs` driver was rewritten around this: a new `reconcile_playing`
keeps each playing animation's start time and a per-avatar monotonic
**activation-order** stamp, begins easing out (rather than dropping) an
animation that leaves the authoritative set and retains it through its
ease-out tail, and (re)activates a new or sequence-changed animation with a
fresh stamp — assigned in **UUID order** within an update, which faithfully
reproduces Second Life's equal-priority quirk (an observer present as each
animation starts sees the last-*started* one win, because the reference pushes
each new motion to the front of its active list; an observer arriving later
starts them all at once from the sorted signalled set, so the highest-UUID one
wins instead — the one stamping rule yields both). `drive_avatar_skeletons`
then samples each playing motion, weights it by `pose_weight`, and blends the
per-joint contributions via `blend_joint`; `PlayState` gained `stopped_at` /
`order` and `AnimationPlayback` a `next_order` counter. To exercise it from
one login, `--play-animation` is now **repeatable** (or comma-separated) so
several animations layer at once. Verified live on OpenSim: the own avatar
with `dance1` + `clap` layered blends cleanly — the clap's arm motion composes
over the dance's full-body pose with no shearing, and the ease-in ramps the
pose up smoothly on start.
