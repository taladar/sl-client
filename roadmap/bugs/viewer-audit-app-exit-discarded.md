---
id: viewer-audit-app-exit-discarded
title: The viewer exits 0 on a failing AppExit
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy-viewer/src/lib.rs:2342` — `let _exit = app.run();`.

Bevy returns `AppExit::Error(NonZero<u8>)` when a plugin requests a failing
exit; `run_session` throws it away, `run_viewer` returns `Ok(())`, and the
process exits **0** on a failed run. This is the crate's only
error-suppression site, and it defeats any script or harness that checks the
exit status.

Two neighbours in the same startup path:

- `:2334-2341` — a failed `create_dir_all` for `--screenshot-dir` is `warn!`ed
  and the run continues, so `ScreenshotPlugin` then fails on every capture
  instead of the run aborting at startup with the real error;
- `:2306` — `--repeat-animation` without `--play-animation` is silently a no-op,
  with no `warn!`.
