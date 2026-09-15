---
id: viewer-own-avatar-vanishes-near-ground
title: The own avatar briefly disappears near the ground while flying or falling
topic: viewer
status: bugs
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
