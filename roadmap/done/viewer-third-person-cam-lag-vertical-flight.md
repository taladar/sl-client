---
id: viewer-third-person-cam-lag-vertical-flight
title: Third-person camera lags behind an avatar flying up or down
topic: viewer
status: done
origin: user report during the spacenav avatar-motion verification (2026-07-28)
refs: [viewer-input-spacenav-avatar-motion, viewer-camera-flycam]
---

Context: [context/viewer.md](../context/viewer.md).

While flying the own avatar **up or down** (PageUp / PageDown or the
SpaceNavigator up axis), the third-person camera visibly **lagged behind** the
vertical motion — the avatar climbed / descended and the camera caught up a beat
later, drifting completely off screen during sustained fast flight until the
flight stopped.

**Two independent causes, both fixed (`camera.rs`):**

1. **World-space pose smoothing trailed the follow.** `apply_pose` eased the
   `smoothed_eye` / `smoothed_focus` toward the desired *world* pose with a
   fixed ~0.1 s half-life. That half-life is meant to glide mode transitions and
   orbit changes, but it also damped the camera's follow of the avatar's own
   translation — a couple of metres of steady-state lag at fly speed.

   Fix: **rigid follow.** For the avatar-follow focus, the focus is re-derived
   from the live avatar every frame and followed with **no** world-space easing;
   only the eye's **offset from the focus** (the orbit / zoom / collision
   geometry) is smoothed. So orbit and zoom still glide, but the camera and
   avatar are a **locked pair** — they can shake together against the world but
   never drift relative to each other (which the user confirmed is the desired
   trade — "much better than keeping the world stable and shaking the avatar").
   A fixed focus point (`lltoolfocus`) keeps the old world-space smoothing (a
   static point has nothing to trail). Pinned by
   `follow_has_no_steady_state_vertical_lag`.

2. **One-frame `GlobalTransform` staleness.** Even with rigid follow the avatar
   still drifted relative to the camera (metres per frame at fly speed, so still
   off screen on the worst frames). `position_camera` read the avatar anchor's
   `GlobalTransform`, which is only recomputed in `PostUpdate`, so in `Update`
   it is **last frame's** world position — the camera was permanently one frame
   behind. Measured `mean |d_root| ≈ 0.21 m/frame`, up to `3.0 m` on correction
   frames.

   Fix: the body-root anchor is a top-level entity, so its **local `Transform`**
   is its world pose and is this frame's value. `position_camera` now reads that
   `Transform` (ordered `.after(drive_avatar_motion)` so it is current), and the
   deep head joint's frame-late `GlobalTransform` is corrected by the anchor's
   own motion this frame (`d_head ≈ d_root`, so the sway relative to the anchor
   is negligible). Zero frame lag.

The flycam is unaffected (it owns its own transform and does not go through
`apply_pose`).

**Follow-up filed:** the residual world-shake during *fast* flight is the
avatar's dead-reckoning translation **rubberband** — a separate root cause,
[[viewer-avatar-dead-reckoning-translation-rubberband]].
