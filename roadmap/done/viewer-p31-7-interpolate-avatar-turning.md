---
id: viewer-p31-7
title: Interpolate avatar turning
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.7. Interpolate avatar turning.** The own avatar's **rotation** is
not smoothed the way its **translation** is (P31.4): a live finding from the
P31.6 run is that turning left / right reads choppy while forward / backward
motion is smooth. The P31.4 avatar dead-reckoner (`drive_avatar_motion`)
advances position from the sim-sent linear velocity + acceleration and spins
the orientation from the angular velocity, but the own avatar's facing is
*driven client-side* by the P31.5 movement controls (a throttled
`SetRotation` at ~20 Hz seeds the sim, which streams the resulting facing
back as terse `ObjectUpdate`s), so between those sparse updates the anchor's
rotation snaps rather than interpolating. Smooth the avatar's orientation
between authoritative rotation updates (slerp toward the target facing, or
fold the client-tracked heading into the render transform continuously) so a
turn looks as fluid as a walk. Viewer-only; unrelated to the P31.6
animations. Reference: Firestorm `LLViewerObject::interpolateRotation` /
the agent's `mDrawable` orientation smoothing. **Done — viewer-only, in
`physics.rs` (`drive_avatar_motion` / `AvatarInterp`).** Root cause matched
the premise: the own avatar's facing arrives only as sparse `ObjectUpdate`s
echoing the client-driven `SetRotation` (essentially zero angular velocity,
so the dead-reckoner's `angular_step` never advanced it between updates), and
both the update-frame snap (`apply_object` writing `body_root_transform`) and
the between-update path wrote the *authoritative* facing straight onto the
anchor — so the rotation stepped while translation eased. Fix: `AvatarInterp`
now carries a `rendered_rotation` (Bevy space) that each frame **slerps**
toward the current authoritative / dead-reckoned facing
(`apply_smoothed_rotation`) with a framerate-independent exponential blend
(`rotation_smoothing_alpha`, `1 - e^(-dt/τ)`, τ = 80 ms) instead of snapping;
the reseed and both dead-reckon exit paths route through it, so the
smoothing spans the update boundary. Chosen over folding the client-tracked
heading in because it is general (smooths **every** avatar's turning, not
just the own one) and needs no cross-module coupling to `movement.rs`. τ =
80 ms converges to the target and leaves no standing lag once turning stops,
so a stationary facing is still exact. Unit-tested (`rotation_smoothing_alpha`
easing curve + slerp convergence-without-snap); the base transform snap was
never the visible artifact so no regression there. **Verified live on
OpenSim** (user-confirmed on screen): ← / → turning now reads as fluid as
the ↑ / ↓ walk. Only the *base* facing is smoothed — the reference viewer's
`LLKeyframeStandMotion` lower-body twist to the look direction is still
P31.8.
