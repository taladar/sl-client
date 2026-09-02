---
id: viewer-capture-layers
title: UI, HUD and gizmos as independent switches in both viewers' captures
topic: viewer
status: done
origin: user request (2026-09-02), while reviewing viewer-screenshot-fixed-resolution
points: 3
refs: [viewer-screenshot-fixed-resolution, test-firestorm-crosscheck-runner,
  test-firestorm-crosscheck-report]
---

Context: [context/testing.md](../context/testing.md).

[[viewer-screenshot-fixed-resolution]] made a captured frame hold the world
alone, as a side effect of how the size was pinned. That is the right
*default* and the wrong *rule*: the HUD is a thing the two viewers draw,
and a cross-check that can never put it in a frame cannot compare it.

So each layer of the composited frame is its own switch, in both viewers,
and none of them is tied to the resolution:

| layer | sl-client | Firestorm |
| --- | --- | --- |
| UI | `--capture-ui` | `SL_VIEWER_CAPTURE_UI` |
| HUD attachments | `--capture-hud` | `SL_VIEWER_CAPTURE_HUD` |
| edit gizmos | `--capture-gizmos` | `SL_VIEWER_CAPTURE_GIZMOS` |
| size | `--capture-size WxH` | `SL_VIEWER_CAPTURE_SIZE` |

Every switch also reads the matching `SL_VIEWER_CAPTURE_*` environment
variable in sl-client, so one env block still configures both viewers, and
all four default to off / 1080p. The size variable was renamed from
`SL_VIEWER_WINDOW_SIZE` in both: it never sized the window, and a name
that says otherwise is how the window ends up being blamed for a frame.

**How each viewer routes a layer** is not symmetric, and the asymmetries
are the parts worth knowing.

sl-client composites four cameras (world 0, gizmos 1, HUD 2, and the UI on
the HUD's camera, `bevy_ui` drawing on `IsDefaultUiCamera`). Every camera
whose layer was asked for is pointed at the off-screen capture target; the
rest keep the window, where a run can still be watched. The one exception
is the HUD and the UI, which are one camera and cannot be routed apart: a
run that asks for exactly one of them **hides** the other for the run. The
gizmo overlay's camera lives in `sl-viewer-edit`, above the harness, so
the routing is driven by a `world_api::OverlayCamera` marker each overlay
puts on its own camera rather than by the harness knowing them all.

Firestorm passes `show_ui` / `show_hud` to `saveSnapshot`, which already
takes them separately. Its gizmo equivalent is not a camera but the
world-pass editor overlays — selection silhouettes, highlights and
beacons, which survive `show_ui = false` — so the switch forces those
settings off unless the run asked for them.

Two traps found while wiring it, both silent:

- **A UI capture is clamped to the window and scaled.** The reference
  snapshot path cannot render the UI at any size but the window's
  ("Scaling of the UI is currently *not* supported"), so it clamps the
  requested size and scales the grab down. A 4K window captured at 1080p
  yields a half-size UI, while sl-client's UI lays out at the capture size
  — the two are not comparable. Firestorm now asks the window to match the
  capture size **only** for a UI run, and says loudly when the window
  manager declines. A world-only run still never resizes: it renders into
  its own scratch target, and shrinking the window would cost a watchable
  run for nothing.
- **A width that is not a multiple of 4 is not the width you get.**
  `rawSnapshot` pads it (`image_width += (image_width * 3) % 4`, a BMP
  row-alignment hack that runs whatever the format), so Firestorm's frame
  comes out wider than sl-client's, which is exact. Both viewers now warn
  rather than refuse; the sizes anyone actually uses are all multiples of
  four.

Verified against the fake grid: frames are exactly the requested grid at
two different sizes, world-only frames hold the world and nothing else,
and the layer switches route each camera independently.
