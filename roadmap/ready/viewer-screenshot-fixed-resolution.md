---
id: viewer-screenshot-fixed-resolution
title: Pin the capture resolution, so two viewers can be compared at all
topic: viewer
status: ready
origin: Firestorm cross-check harness plan (2026-09-01)
points: 2
refs: [viewer-screenshot-wait-for-quiescence]
---

Context: [context/testing.md](../context/testing.md).

`--screenshot-dir` captures `Screenshot::primary_window()`, so a frame is
whatever size the window happened to be — Bevy's default, or whatever a
tiling WM handed us. There is no resolution or window-size flag anywhere
in the workspace. That is fine while the only consumer is this viewer's
own pixel oracles, which classify colours at CPU-projected points and do
not care about absolute size; it is fatal the moment a frame is put beside
Firestorm's, because two images of different dimensions cannot be diffed,
tiled into a contact sheet, or compared at a named pixel.

Add `--window-size WxH` with env `SL_VIEWER_WINDOW_SIZE`, parsed like the
other capture knobs and applied to the primary window before the first
capture. Firestorm's harness already reads the same variable and calls
`LLViewerWindow::reshape` with it, so one env block sizes both viewers.

Reject a malformed value loudly rather than falling back to the default:
a silent fallback produces a full run of unusable frames whose only
symptom is that the diff step later refuses them.

Refs [[viewer-screenshot-wait-for-quiescence]] — the capture must still
wait for quiescence *after* the resize, since a reshape re-triggers
texture and LOD work.
