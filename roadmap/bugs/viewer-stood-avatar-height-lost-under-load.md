---
id: viewer-stood-avatar-height-lost-under-load
title: Under load the viewer reports a stood-up avatar at 25.95 m, not where the grid stood it
topic: viewer
status: bugs
origin: viewer-automation-ground-reveal-eye-under-terrain commit hook (2026-11-10)
refs: [gridspec-sit-stand]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`e2e_sit`'s `the_avatar_sits_stands_and_is_refused_as_each_grid_does` failed
in the commit hook's full suite (process backend), and fails every time when
starved:

```text
ROADMAP_COORD_BYPASS=1 systemd-run --user --scope -q -p CPUQuota=15% \
  cargo nextest run --release -p sl-client-bevy-viewer --test e2e_sit
```

| flavour | grid stands the avatar at | viewer reports, for the whole 60 s wait |
| --- | --- | --- |
| Second Life | 131.32, 124.0, 26.38 | 131.32, 124.0, **25.95** |
| OpenSim | 133.06, 124.0, 26.97 | 133.06, 124.0, **25.95** |

Four starved runs failed: two with the ground-reveal camera change and two at
`95581fd6` without it. Unstarved, the test passes alone.

x and y are right; z is the same 25.95 m on both flavours, so it does not
come from the stand position. It is half the avatar's 1.9 m height above 25 m,
which suggests an avatar ground floor (`physics.rs` `avatar_ground_floor` /
`avatar_collision_floor`) or the dead-reckoner settling the stood avatar onto
ground the fake grid never stands it on. Unstarved, the test may only pass
because it reads the position before the prediction moves it.

## What to do

- Run starved with `SL_VIEWER_LOG_AVATAR_GROUND=1` and debug logging for
  `sl_viewer_world_view::physics` and `sl_viewer_world_avatar::avatars`. Find
  which floor or prediction writes 25.95, and what the stand update carried:
  velocity, acceleration and collision plane.
- Decide which side is wrong: the viewer predicting a fall the grid does not
  make, or the fake grid standing an avatar in the air without the motion a
  live grid would send. Then fix it, so the starved run passes.
