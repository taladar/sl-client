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
from `sl-viewer-spacenav/src/lib.rs` / `sl-viewer-world-view/src/camera.rs`:

- per-axis dead-zone (sub-threshold input moves nothing);
- per-axis scale in flycam-function order (`FlycamAxisSettings`);
- the feathering ramp over frames;
- auto-leveling easing roll back to horizontal;
- `toggle_flycam` edge-triggering the mode switch;
- the flycam camera pose deltas per axis (translate ×3, rotate ×3).

Doubles as the executable spec the deferred cross-platform reader
([[viewer-input-spacenav-crossplatform]]) must satisfy.

## Audit 2026-10-15

Some of this exists. `sl-viewer-spacenav/src/lib.rs` tests the avatar-walk
mapping (a centred device is inert, forward / up by sign, the yaw ramp, the
run threshold). `sl-viewer-world-view/src/camera.rs` tests the toggle
(`a_toggle_flycam_request_enters_and_leaves_the_flycam`, through
`ToggleFlycam` rather than the device button) and that an idle, levelled
flycam stops writing its transform. What remains is `drive_flycam` itself
under injected axes: the per-axis dead zone, the per-axis scale order, the
feathering ramp, auto-levelling easing the roll back, and the per-axis pose
deltas.
