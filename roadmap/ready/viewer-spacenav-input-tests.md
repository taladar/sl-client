---
id: viewer-spacenav-input-tests
title: 6-DOF input reactions via the SpacenavInput seam
topic: viewer
status: ready
origin: user request (2026-07) — test space navigator reactions
points: 3
refs: [viewer-input-spacenav-crossplatform]
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

`SpacenavInput` is a plain resource consumers only read — the ideal
injection seam; no device, evdev, or feature flag needed in tests. Inject
axis vectors and assert `drive_flycam` applies the reference semantics
from `spacenav.rs`/`camera.rs`:

- per-axis dead-zone (sub-threshold input moves nothing);
- per-axis scale in flycam-function order (`FlycamAxisSettings`);
- the feathering ramp over frames;
- auto-leveling easing roll back to horizontal;
- `toggle_flycam` edge-triggering the mode switch;
- the flycam camera pose deltas per axis (translate ×3, rotate ×3).

Doubles as the executable spec the deferred cross-platform reader
([[viewer-input-spacenav-crossplatform]]) must satisfy.
