---
id: viewer-p19-1
title: FPS + frame-time overlay
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 19 — Diagnostics HUD (FPS + pipeline status)
---

Context: [context/viewer.md](../context/viewer.md).

The rendering-fidelity phases below drive the fetch/decode pipeline much
harder, so the first new phase gives us the instruments to see it: an FPS /
frame-time readout and a live texture/mesh pipeline status panel. Reuses the
Phase 11 chat-overlay `bevy_ui` `Text` pattern (`chat.rs`).

**P19.1. FPS + frame-time overlay.** Add Bevy's
`FrameTimeDiagnosticsPlugin`; render a `bevy_ui` text panel (the persistent
absolute-positioned `Text` node pattern from `chat.rs`) showing FPS,
frame-ms, and entity / draw counts. Reference: `LLViewerStats` /
`LLFastTimerView` / `LLPerfStats`. **Done:** new viewer module
`diagnostics.rs` — the viewer adds `FrameTimeDiagnosticsPlugin` +
`EntityCountDiagnosticsPlugin`, a persistent top-left `Text` node (clear of
the bottom-left chat overlay), rewritten each frame with the smoothed
`FPS` / `FRAME_TIME` / `ENTITY_COUNT` diagnostics and a `draws` figure from
the live `Mesh3d` instance count (a coarse per-frame draw-call gauge; Bevy
has no draw-call diagnostic without the GPU-timing `RenderDiagnosticsPlugin`).
Verified live on OpenSim: the overlay reads e.g. `FPS 60  (16.6 ms)` /
`entities 1522  draws 1068`.
