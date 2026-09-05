---
id: viewer-floater-interaction-tests
title: Floater chrome under a real pointer
topic: viewer
status: ready
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 5
blocked_by: [viewer-ui-interaction-harness, viewer-floater-registry]
---

Context: [context/viewer.md](../context/viewer.md).

Drive `floater.rs`'s real observers headlessly:

- title-bar `Pointer<Drag>` moves the floater — assert
  `FloaterGeometry.position` tracks the drag, clamped to the viewport;
- resize-handle drags respect `min_size` and the content reflows without
  `layout_violations`;
- dock/minimize/close buttons emit their `FloaterOp`s;
- press-anywhere brings to front (`FloaterZTop` ordering);
- `floater_persist.rs` round-trips geometry.

Swept over the whole `FLOATERS` registry so a new floater inherits chrome
coverage by registering.
