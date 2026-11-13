---
id: viewer-driver-pie-wait-wall-clock
title: Under load the driver's pie wait gives up on a pie that is still opening, and the replayed click never settles
topic: viewer
status: bugs
origin: viewer-stood-avatar-height-lost-under-load starved runs (2026-11-14)
refs: [viewer-stood-avatar-height-lost-under-load, viewer-double-click-window-frame-clock]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

One of four `e2e_sit` runs at a 15% CPU quota (in-process backend, Second
Life flavour, about 3 frames a second) failed opening the seated seat's pie
to stand up:

```text
WARN sl_viewer_driver::world: a right click opened no pie menu; playing it
  again what=open the pie of object[name=Seat] clicks=1    (45.5 s, 66.0 s)
Error: … action: "right_click on object[name=Seat]", error: WorldTimedOut {
  failed_check: Some(Stable), … frames: 194, millis: 60092 }
```

The failure screenshot shows the avatar still seated **with a pie open over
the seat**: the first click had opened one, later than the driver looked. The
replayed click's aim then reframed the camera (`camera: framing … from
3.21 m` at 47.1, 68.5 and 116.9 s) and never saw two frames agree.

## Suspects

- `open_a_pie` (`sl-viewer-driver/src/world.rs`) waits `PIE_OPENS_WITHIN`,
  3 s of **wall clock**, which is nine frames at 3 fps: the same frames-
  versus-time hazard [[viewer-double-click-window-frame-clock]] fixed for
  double clicks. A wait counted in viewer frames, or one that looks for an
  open pie before replaying, would not replay over a pie on its way.
- Why the replayed aim never settles with a pie open (the seated avatar as
  the seat's child in the aim's box, or the reveal's glide) is unexplained.

## Repro

```sh
ROADMAP_COORD_BYPASS=1 systemd-run --user --scope -q -p CPUQuota=15% \
  cargo nextest run --release -p sl-client-bevy-viewer --test e2e_sit
```

Rare: three further runs passed.
