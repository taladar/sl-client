---
id: viewer-harness-status-counts-before-last-frame
title: The screenshot harness writes its status before the last frame is saved
topic: viewer
status: bugs
origin: sl-crosscheck --scenario scripts (server-fake-grid-scripted-scenario, 2026-11-13)
refs: [server-fake-grid-scripted-scenario]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

A 30-frame `sl-crosscheck --scenario scripts` run left 30 PNG files in the
sl-client directory and a `harness-status.json` reading
`{"ok": true, "reason": "complete", "frames_written": 29,
"frames_expected": 30}`, which `sl-crosscheck` printed as
`ok — complete (29/30 frames)`. So the file contradicts itself and the disk.

`sl-viewer-world-view/src/screenshot.rs`: once `index` reaches `max_frames`
the schedule waits only for `pending_saves` (the `ScreenshotSaveTask`
entities) to drain before `write_status()`. The last frame's save task is
spawned by the capture's readback observer a frame or more after the capture
is requested, so there is a window where the last frame is captured, not yet
read back, and no save task exists. The status is written in that window and
the PNG lands during the logout grace.

## Wanted

The status is written once every captured frame is written (or has failed),
so `frames_written` matches the disk. An `ok: true, complete` status with
fewer frames written than expected should not be possible: either wait, or
refuse to call it complete.
