---
id: viewer-audit-sit-camera-gating
title: The scripted sit camera arms on any SitResult and never clears forced mouselook
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

Two defects in `sl-viewer-world-view/src/sit_camera.rs`:

- `:120` — the scripted sit camera arms on **any** `SitResult`, with no
  "actually sitting" gate (no `autopilot` check, though `SitResult` carries one;
  the reference conjoins `isSitting()` on both branches).
  `clear_sit_camera_on_stand` (`:156`) only fires on a seated-to-unseated edge,
  so a **cancelled** sit welds the camera to the seat indefinitely.
- `:139` — `forced_mouselook` is write-only: set inside `if *force_mouselook {`
  with no `else` clearing it. A sit-to-sit hand-off (A forces, B does not, no
  stand between) leaves it armed, and standing from B then steals the user's own
  mouselook choice.

`sit_camera.rs` has zero tests; `ingest_sit_result` is testable as-is with
`MinimalPlugins` + `add_message::<SlEvent>()`, the pattern `session.rs:548`
already uses.
