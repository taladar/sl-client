---
id: viewer-own-avatar-vanishes-near-ground
title: The own avatar briefly disappears near the ground while flying or falling
topic: viewer
status: done
origin: aditi fly-and-fall runs for [[viewer-crossing-movement-locks-up]]
  (2026-09-15)
refs: [viewer-crossing-movement-locks-up, viewer-own-motion-timed-stops,
  viewer-avatar-falls-through-ground]
---

Context: [context/viewer.md](../context/viewer.md).

On aditi the **own** avatar relatively often disappears for a moment when it is
near the ground — seen while flying low and while dropping out of flight onto
the ground, in both of two consecutive sessions. It comes back on its own.

What is known:

- Not seen before, but flying and falling are not a routine part of testing, so
  that says little about when it started.
- Both sessions ran a build carrying [[viewer-own-motion-timed-stops]], which
  sends `AgentAnimation` stops and `FINISH_ANIM` for the own avatar's finished
  motions. Nothing in it touches rendering or the avatar's position, but it has
  not been ruled out: an A/B on aditi with that change reverted is the cheapest
  first step.
- Not seen on the local OpenSim in three fly / fall / cliff-crossing sessions
  the same day — though nobody was watching for it there.

Suspects, none checked yet: the avatar's render bounds or culling while its
dead-reckoned position is below the terrain the probe resolves (cf.
[[viewer-avatar-falls-through-ground]]); a pose-driver frame with no motion
drivable (a landing animation swapped out before its asset decoded) leaving the
body un-posed or zero-weighted; or the GPU-avatar path dropping the avatar for a
frame range during an animation-set change.

Repro to try: aditi, fly low over flat ground, then stop flying a few metres up
and land; watch the own avatar through the descent and landing. Record the time
so it can be matched against `SL_VIEWER_LOG_LOCOMOTION=1` output.

## Cause (2026-09-15)

A frustum cull on a **stale box**. Phase 5 of the GPU-avatar work culls each
avatar by a posed bound the GPU writes and the CPU reads back; the readback
lands about **three frames** late, and the box was in world space, so it stood
where the avatar had been. A 1.25 m flesh + motion margin covered a walk. A
fall does not fit in it: aditi's fall reaches ~37 m/s, 1.5 m a frame at the
~25 fps of that scene, so the box sat 4–5 m above the body, cleared the top of
the third-person view, and every part was culled while the avatar was on
screen.

Measured with a new frame-by-frame watch
(`SL_VIEWER_LOG_OWN_AVATAR_VISIBILITY=1`): in the two falls where the vanish was
seen, and only there, all 45 shown parts missed the main camera's frustum
while a sphere round the body root was inside it, for 1–10 frames at a time;
the read-back box at frame N was exactly the root of frame N−3
(`root_outside` 4–5 m). `ViewVisibility` never showed it — it is the union over
every view, and the reflection-probe camera kept seeing the avatar — which is
why the watch's first version, keyed on it, fired only at login.

Not the suspects listed above: no respawn, no hidden part, no non-finite pose,
no jump, and the camera stayed ~3.4 m out. The motion-stops change is
unrelated. The first session also saw it while flying up, but that watch could
not yet tell a main-view cull apart, and the second reproduced it only in
falls — so only falls are measured. The same stale box explains any fast
vertical motion.

## Fix

- `pose.wgsl`: the `bounds` pass writes the box **relative to the translation
  of the root it posed under**.
- `apply_gpu_avatar_bounds` places it on the root the slot published **this**
  frame (`GpuAvatarPoseFeed::root_translation`), so the box moves with the
  avatar; only the pose itself (a limb, a turn) is still three frames old,
  which is what the 0.5 m motion margin is now documented to cover.
- Test `the_cull_box_follows_a_falling_avatar` applies one read-back box at the
  root it was posed under and at the current one, under a third-person camera:
  the first is culled (the bug), the second seen.
- The watch stays as a diagnostic (`gpu_avatars/own_watch.rs`): culled against
  the main camera, camera inside the body, root jump, respawn, plus a 1 Hz
  timeline and each animation-set change.

**Live (aditi, same avatar and spot):** two climbs and two falls, from ~95 m
and ~135 m, the second reaching 2.4 m a frame (~46 m/s) — faster than the
falls that vanished. The user saw no vanish; the watch logged no cull, and the
placed box held the root in every frame (`root_outside` 0). The watch's jump
trigger, then a fixed 3 m a frame, fired on those fast low-fps frames, so it is
now a speed (100 m/s over the frame).
