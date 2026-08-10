---
id: viewer-perf-settings-save-offthread
title: Settings persistence writes on the IO task pool
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: []
---

Context: [context/viewer.md](../context/viewer.md).

The in-session settings persistence paths — the periodic floater-geometry
flush (`flush_floater_settings`), table column-width drags, the preferences
apply, and the friends-list sort change — called `ViewerSettings::save()`,
which synchronously serialized and **wrote both scope files on the frame
thread** (blocking disk I/O in a frame).

Fix: `sl-settings` splits `save_scope` into `serialize_scope` (pure, no
filesystem) + the write, and the viewer gains
`ViewerSettings::save_async()`: serialize both scopes on the frame thread
(the TOML is small) and write the files on one detached `IoTaskPool` task,
with write failures logged from the task (never hidden). All four
in-session call sites switched.

The logout / exit path (`session.rs` quit save) deliberately keeps the
synchronous `save()` — a detached write racing process exit could be lost.

Verify: move/resize a floater, wait the 30 s flush interval, confirm the
account `settings.toml` updates and the `settings: saved …` log line comes
from a task-pool thread.
