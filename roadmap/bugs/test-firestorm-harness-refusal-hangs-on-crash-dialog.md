---
id: test-firestorm-harness-refusal-hangs-on-crash-dialog
title: A Firestorm harness refusal hangs on a crash dialog until the deadline
topic: test
status: bugs
origin: viewer-vintage-ui-chrome-crosscheck live run (2026-09-27)
refs: [test-firestorm-crosscheck-runner, test-firestorm-harness-skin-selection,
       viewer-vintage-ui-chrome-crosscheck]
---

Context: [context/testing.md](../context/testing.md).

When the patched Firestorm's harness (`fstestharness.cpp`, fork branch
`test-harness`) refuses its configuration, it does so with `LL_ERRS`. That is
the viewer's *crash* path: it opens the modal `OSMessageBox` "We are sorry, but
Firestorm has crashed and needs to be closed", and that dialog waits for a
person. An unattended run therefore does not exit. It sits on the dialog until
`sl-crosscheck`'s deadline asks it to quit, which on a three-frame run took
**271 s**. It writes no `harness-status.json`, so the run reads as "did not
happen", with the real reason buried in `viewer.log`.

Seen live: a relative `--run-dir` (fixed in the same task: `RunDirs::new` now
makes the root absolute) meant `--credentials` named a file that was not there
after the launcher changed directory. Line 305 refused it, and the refusal
became a four-and-a-half-minute hang.

Every harness refusal takes this path. `LL_ERRS` sites in `fstestharness.cpp`:
`--credentials`, `--gridfile`, `--camera-position`, `--camera-look-at`,
`SL_VIEWER_CAPTURE_SIZE`, and the three skin/theme refusals.

## Wanted

- A harness refusal ends the process promptly with a non-zero exit, and with
  no modal dialog. That means a harness-specific fatal path, not `LL_ERRS`: log
  the reason, write the status, exit.
- Before exiting, it writes `harness-status.json` with `ok: false` and the
  refusal as `reason`, if a `--screenshot-dir` was parsed. Then the runner
  prints "FAILED: --credentials: cannot open …" instead of "NO STATUS". Parse
  `--screenshot-dir` first, so that every later refusal has somewhere to
  report to.
- A refusal that comes before the screenshot directory is known still exits
  promptly. The runner's "no status" line is then accurate.

## Done when

A run with a deliberately bad `--credentials` path, bad skin name, or bad
`SL_VIEWER_CAPTURE_SIZE` exits within seconds. The runner reports that refusal's
own words as the Firestorm half's failure reason.
