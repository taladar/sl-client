---
id: viewer-audit-app-exit-discarded
title: The viewer exits 0 on a failing AppExit
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`run_session` threw away what `app.run()` returned (`let _exit = app.run();`),
`run_viewer` returned `Ok(())`, and the process exited **0** on a failed run.
Bevy writes `AppExit::Error` for a plugin that will not build and for a
renderer thread that panicked (`bevy_render`'s `pipelined_rendering.rs` does
exactly that), so the viewer's most violent failures were reported to any
script or harness as success.

## The fix

`run_session` returns `Result<LoginOutcome, Error>` and maps `AppExit::Error`
to a new `Error::AppFailed`. Both callers propagate it, so `run()` — and the
process — fails.

The `LoginOutcome` is still taken out of the world before the exit is judged,
but reported only on a clean exit: an app that failed has not "stopped on an
MFA challenge", and handing the retry loop a challenge on the strength of a run
that never got that far would send it round the login loop again.

`Error::AppFailed` carries the code the app chose, but the process exits `1`.
`main` returns normally rather than calling `std::process::exit`, which would
skip the tracing guards' flush — losing the log that explains the failure in
the very act of reporting it.

## The two neighbours in the same startup path

- A failed `create_dir_all` for `--screenshot-dir` was `warn!`ed and the run
  continued, so `ScreenshotPlugin` then failed on every single capture instead
  of the run aborting at startup with the real error. It is now
  `Error::ScreenshotDir` and aborts — before any window opens.
- `--repeat-animation` without `--play-animation` was silently a no-op. It now
  `warn!`s, matching the `--capture-*` and `--scene-dump` warnings beside it.

## Taken beyond the item

The same defect, in the same crate, in the two binaries used for the fastest UI
and render checks: `gallery::run` and `render_gallery::run` both discarded
their `AppExit` into a `main` returning `()`. Both now return Bevy's own
`AppExit`, and both binaries' `main` returns it — `AppExit` implements
`Termination`, so a failing gallery run reaches a harness's exit-code check
with its own code intact. `render_gallery::run` also answered a mistyped
`--scene` with a bare `return`, exiting 0 after printing the list; it now
returns `AppExit::error()`.

And the error text itself. `main` returning a `Result` prints the error's
`Debug`, which buries the sentence saying what went wrong inside the struct
carrying it. It now renders `Display` and walks the source chain, through
`tracing` at `error!` — the level the crate already uses for a launch that
fails outright, so the failure lands in the same log as everything leading to
it and shows even at `RUST_LOG=error`.

## How it was verified

Both new failure paths were run, and neither opens a window:

```text
$ sl-client-bevy-viewer-scenes --scene definitely-not-a-scene
ERROR … no scene named `definitely-not-a-scene`. Known scenes: prim-box, …
EXIT STATUS: 1

$ sl-client-bevy-viewer --credentials … --screenshot-dir /proc/cannot-create-here
ERROR sl_client_bevy_viewer: could not create the screenshot directory
ERROR sl_client_bevy_viewer:   caused by: failed to create directory
  `/proc/cannot-create-here`: No such file or directory (os error 2)
EXIT STATUS: 1
```

Both were exit `0` before. The second was re-run under `RUST_LOG=error` to
confirm the report survives the filter.

Not verified by an automated test: the paths that matter live in `main` and in
`app.run()`'s return, which no unit test can reach without running the app.
What a future harness change should re-check is the pair above.
