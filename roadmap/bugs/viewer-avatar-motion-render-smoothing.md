---
id: viewer-avatar-motion-render-smoothing
title: Smooth the avatar's rendered position (dead-reckoning jitter)
topic: viewer
status: bugs
origin: surfaced by the following third-person camera in the camera-system pass
---

Context: [context/viewer.md](../context/viewer.md).

The own avatar's rendered body **vibrates** with a high-frequency, ~cm
whole-body jitter. The cause is the dead-reckoner (`crate::physics`
`drive_avatar_motion`, P31.4) **snapping** the render `Transform` to each server
update: the simulator streams the avatar as **terse**
`ImprovedTerseObjectUpdate`s with 16-bit **quantized** positions (~cm steps) at
a high rate, so snapping to each one hops the body between quantised positions
every update.

It was invisible with the old detached debug fly-camera (which never followed
the avatar); the new head-following third-person camera
([[viewer-camera-third-person-orbit]]) makes it obvious.
**The camera work did not cause it** (`physics.rs` was untouched); it only
revealed it.

Fix: ease the rendered position toward the authoritative one (a
critically-damped smooth, or a short low-pass) so the terse-update quantisation
is filtered, rather than snapping on `motion.is_changed()`. Keep the P31.4
dead-reckoning / phase-out / ground-floor behaviour; only the *render* position
needs smoothing. Mind the region-crossing rebase and the sit / vehicle cases.

Reference (Firestorm, read-only): `indra/newview/llviewerobject.cpp`
interpolation, `indra/newview/llvoavatar` position smoothing.
