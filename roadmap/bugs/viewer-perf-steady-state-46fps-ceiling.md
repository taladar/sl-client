---
id: viewer-perf-steady-state-46fps-ceiling
title: Steady-state frame rate caps at ~46 fps on the local grid (was 60)
topic: viewer
status: bugs
origin: A/B verification runs for the run-condition gating pass (2026-08-10)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

During the [[viewer-perf-inventory-view-visibility-gate]] verification
(2026-08-10, `performance` branch, local OpenSim, 3840x2160 window — the
same screen earlier sessions used), the viewer never reached 60 fps:
~44 fps at t=40 s and still ~46 fps at t=95 s, well past the cold-login
streaming burst. Past runs on this scene reportedly sat at 60 fps.

Ruled out by measurement (traces in that session's scratchpad, numbers
in the two gating commits):

- **The visibility-gating change**: alternating gated/ungated A/B runs
  (`SL_VIEWER_DISABLE_PANEL_GATE` toggle, same binary, same conditions)
  show identical visible-phase fps (46.7 / 47.1 / 45.7) and identical
  Render-schedule medians (19.78 / 19.78 / 20.20 ms).
- **Tracy overhead**: with no profiler attached the status-bar readout
  still shows 44-46 fps.
- **Compositor present-throttling**: distinct symptom (~1000 ms
  `vkQueuePresentKHR` blocks while the window is occluded); the 46 fps
  phases have 0.1 ms presents.

Steady-state frame anatomy (visible phase, t=30-60 s, gated run b2):
both threads sit at ~21 ms, so the frame is co-limited —

- main thread: `Main` schedule ~14.8 ms (PostUpdate alone 6.6 ms) +
  RenderExtractApp ~6.9 ms ≈ 21.7 ms;
- render thread: `Render` schedule ~20.2 ms (render_system 7.4 ms,
  camera_driver 4.2 ms, the rest parallel prepare/queue work, e.g.
  `queue_shadows` ~2.3 ms across many workers).

## Investigation plan

- Establish when it regressed: rerun the same measurement (status-bar
  fps + a tracy capture, window visible) on earlier `performance`-branch
  commits — candidates since the last known-60 observation include the
  terse-update fast path, the bevy_flair patch pin, the session network
  thread, and the frame-spreading pass — and bisect the first ~21 ms
  commit.
- Then split main- vs render-thread: which side moved? PostUpdate at
  6.6 ms and extract at 6.9 ms are the main-thread chunks worth
  per-system ranking; on the render side rank the prepare/queue systems
  ([[viewer-profiling]] workflow — per-instance wall-clock on the gating
  thread, never summed self-time).
- Confirm any fix at the status bar (60 fps restored) AND in the trace
  (both threads' schedule medians), window visible and focused.
