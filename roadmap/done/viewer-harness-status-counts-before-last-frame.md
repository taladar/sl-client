---
id: viewer-harness-status-counts-before-last-frame
title: The screenshot harness writes its status before the last frame is saved
topic: viewer
status: done
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

## Done (2026-11-14)

`ScreenshotSchedule` now counts every capture's outcome itself: `written`, and
a new `failed` for a frame that could not be decoded from its readback (until
now only logged) or failed its write. The run waits for
`index - written - failed` to reach zero (`drained`) before it writes the
status and logs out, in both the normal and the failure branch, instead of
waiting for the save-task entities, which only exist once a readback arrives.
A capture that never reads back is written off after 30 s
(`DRAIN_TIMEOUT_SECS`) as a failure naming how many frames were lost, so the
run fails rather than hangs. And `status()` refuses `ok: true` with fewer
frames written than expected (`only N of M frames were written`), whatever
else the run recorded.

Unit tests cover the frame not yet read back, the short set, a failed frame
as an outcome and the drain timeout. A one-sided
`sl-crosscheck --scenario scripts --only sl-client` run reported
`complete (30/30 frames)` with 30 frames on disk, and its log has `frame_029`
saved before the logout.
