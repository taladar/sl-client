---
id: viewer-perf-pipeline-overlay-text-churn
title: Pipeline debug overlay rewrites its Text unconditionally every frame
topic: viewer
status: ready
origin: per-frame ECS churn audit (2026-08-09)
refs: [viewer-perf-ui-layout-gate-open-widget-churn]
---

Context: [context/viewer.md](../context/viewer.md).

`update_pipeline_overlay` (now `sl-viewer-world-scene/src/diagnostics.rs`,
Update, every frame with no run condition) does
`*text = Text::new(format_pipeline(…))` unconditionally each frame while the
overlay is toggled on (still so at the 2026-10-15 audit) — no equality check,
and the node is not `FixedSlotContentSize`. The resulting `ContentSize` churn
trips `ui_perf::ui_layout_dirty` → full relayout + parley reshape every frame
the panel is visible. Off by default (key-toggled `PipelineOverlayVisible`), so
it only bites a developer with the overlay open — but that is exactly when frame
times are being read, skewing the measurement.

Fix: equality-guard the text write (the string rarely changes), mark the node
`FixedSlotContentSize` with a content-independent width per the
`status_bar.rs:502-565` pattern, and/or `run_if` on `PipelineOverlayVisible`.
The visibility writes in the same system are already equality-gated.
