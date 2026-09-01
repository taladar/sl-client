---
id: test-firestorm-crosscheck-runner
title: Run both viewers against one fake grid and collect their artifacts
topic: test
status: blocked
origin: Firestorm cross-check harness plan (2026-09-01)
points: 5
refs: [test-firestorm-fake-grid-crosscheck]
blocked_by: [viewer-screenshot-fixed-resolution, test-fake-grid-fixed-port-scenario]
---

Context: [context/testing.md](../context/testing.md).

The driver. Start `sl-fake-grid` on a fixed port with a named scenario,
launch this viewer and Firestorm against it with the same resolution,
camera and day position, wait for both, and collect
`<run>/<viewer>/frame_NNN.png`, `scene.json` and `harness-status.json`.

Firestorm gained the matching surface in its own tree: `--credentials`
(the TOML file this workspace already uses), `--avatar`, `--gridfile`,
`--screenshot-dir`, `--camera-position`, `--camera-look-at`,
`--scene-dump`, plus `SL_VIEWER_SCREENSHOT_{DELAY,INTERVAL,FRAMES}`,
`SL_VIEWER_SKY_DAY_POSITION` and `SL_VIEWER_WINDOW_SIZE`. Three things
about driving it are not guessable:

- **`FIRESTORM_X64_USER_DIR=<tmpdir>`** per run. Without it a run shares
  settings, cache, logs, `grids.user.xml` and the credential store with
  the user's real session, and two concurrent runs fight over them.
- **`--grid <ipv4:port>`, never `--loginuri`.** `CmdLineLoginURI` is dead
  code in the OpenSim build of Firestorm — declared, mapped, and read by
  nothing. `--grid` with an unknown name is treated as a host and resolved
  through `GET /get_grid_info`, which the fake grid serves.
- **`--multiple`**, or a second instance refuses to start.

**Teardown is the part that will bite.** Both viewers must log out, not be
killed: a session the simulator still believes is logged in makes the
*next* run fail to log in, and that failure looks exactly like a viewer
bug. This viewer already logs out and allows a grace period before
exiting; Firestorm's harness does the same (`requestQuit` → wait →
`fastQuit`, never `forceQuit`). So the runner asks a viewer to quit, waits
out its logout grace, and only then escalates — and never `SIGKILL`s a
logged-in viewer as a first move. Firestorm's own `--quitafter` is
unusable here for exactly this reason: it calls `forceQuit()`, which sends
no `LogoutRequest`.

Read `harness-status.json` rather than the exit code to decide whether a
run happened at all. "The viewers differ" and "the run did not happen"
must never be reported the same way.
