---
id: viewer-debug-screenshot-offthread-save
title: Offload the debug screenshot PNG encode off the main thread
topic: viewer
status: done
origin: viewer-clouds-sun-occlusion-horizon-contact capture runs (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact]
---

Context: [context/viewer.md](../context/viewer.md).

The `--screenshot-dir` / `SL_VIEWER_SCREENSHOT_*` debug harness
(`screenshot.rs`) saves each frame with Bevy's built-in `save_to_disk`. On
native that observer does `img.save_with_format(...)`
**synchronously on the main thread** — a full-resolution PNG encode + write per
captured frame — while only the GPU readback is async. Each captured frame
therefore stalls the main thread, and time-based animations (the water surface,
driven by `time.elapsed_secs()`) **jump** on the catch-up frame — the "water
briefly accelerates then normal" artifact seen during capture runs. It only
affects capture runs, not normal use.

**Work:** save off the main thread — spawn the PNG encode + write onto
`IoTaskPool` (as the user-facing Snapshot floater does) instead of Bevy's
synchronous `save_to_disk`, so captures don't hitch the frame and the harness
better reflects live behaviour. Purely a debug-tooling improvement.

**Resolution:** `screenshot.rs` now observes `ScreenshotCaptured` with
`save_off_thread`, which decodes the frame to RGB on the frame thread (dropping
the HDR-brightness alpha, as `save_to_disk` did) and hands the PNG deflate +
disk write to `IoTaskPool` as a `ScreenshotSaveTask`. `poll_screenshot_saves`
drains finished writes each frame and logs the saved path / any write error. To
keep a process-exit race from truncating the final PNG(s), `capture_screenshots`
holds the post-capture logout until no `ScreenshotSaveTask` is still in flight.
