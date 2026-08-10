---
id: viewer-perf-media-upload-budget
title: Per-frame byte budget for media-surface uploads
topic: viewer
status: deferred
origin: unbounded-frame-work survey (2026-08-09, performance branch)
---

Context: [context/viewer.md](../context/viewer.md).

`pump_media_engine` mirrors each CEF surface with a **new frame** into its
`Image` (full-frame BGRA memcpy + GPU upload). It already skips surfaces
whose `seen_frame` is current, so the cost is N *animating* surfaces × one
full-frame copy — unbounded only when several media prims play at once.

Deferred until Tracy shows `pump_media_engine` spikes with multiple live
media prims. Mechanism when taken:

- A per-frame byte budget (`SL_VIEWER_MEDIA_UPLOAD_BUDGET_BYTES`, ~16 MB)
  with a **round-robin start offset** across surfaces so one big surface
  cannot permanently starve the others.
- A skipped surface simply keeps `seen_frame` unbumped and picks up the
  *latest* CEF frame on its next turn — frame-skipping is correct for
  25–30 fps media.
- NEVER break the same-size in-place `Image.data` mutation: it is
  load-bearing for `GpuImage`'s `write_texture` reuse (existing bind groups
  stay valid — see the sl-cef notes). Budgeting must skip whole surfaces,
  not split one surface's copy.
- Alternative worth weighing then: double-buffer in the media engine
  thread so the frame thread only swaps pointers; the `Assets<Image>` data
  write (and thus the copy) currently has to happen on the main world.
