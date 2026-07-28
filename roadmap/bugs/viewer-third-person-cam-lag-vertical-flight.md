---
id: viewer-third-person-cam-lag-vertical-flight
title: Third-person camera lags behind an avatar flying up or down
topic: viewer
status: bugs
origin: user report during the spacenav avatar-motion verification (2026-07-28)
refs: [viewer-input-spacenav-avatar-motion, viewer-camera-flycam]
---

Context: [context/viewer.md](../context/viewer.md).

While flying the own avatar **up or down** (PageUp / PageDown or the
SpaceNavigator up axis), the third-person camera visibly **lags behind** the
vertical motion — the avatar climbs / descends and the camera catches up a beat
later, so the framing drifts during sustained vertical flight rather than
tracking the body cleanly.

Likely the pose smoothing in `camera.rs`: `position_camera` eases the
`smoothed_eye` / `smoothed_focus` toward the desired third-person pose with a
fixed `SMOOTH_HALF_LIFE` (~0.1 s) each frame (`apply_pose`). That half-life is
tuned to glide *mode transitions* and orbit changes, but it also damps the
camera's follow of the avatar's own translation, which is most obvious on the
fast, sustained vertical axis (horizontal walking is slower, so the same lag
reads as acceptable).

Investigate:

- Whether the reference (`LLAgentCamera`) follows the avatar position
  **rigidly** and only smooths the orbit/zoom deltas — i.e. the smoothing
  should apply to the orbit offset, not to the avatar-anchored focus point, so
  the camera never trails the body's world position.
- If the focus does need easing, whether it should track the avatar's velocity
  (lead the smoothed point by the avatar's per-frame delta) so a constant climb
  has zero steady-state lag.
- Confirm the flycam is unaffected (it owns its own transform and does not go
  through `apply_pose`).
