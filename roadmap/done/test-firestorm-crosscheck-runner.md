---
id: test-firestorm-crosscheck-runner
title: Run both viewers against one fake grid and collect their artifacts
topic: test
status: done
origin: Firestorm cross-check harness plan (2026-09-01)
points: 5
refs: [test-firestorm-fake-grid-crosscheck]
blocked_by: [viewer-screenshot-fixed-resolution, test-fake-grid-fixed-port-scenario]
---

Context: [context/testing.md](../context/testing.md).

Both blockers are cleared. [[test-fake-grid-fixed-port-scenario]]
(2026-09-02) shipped `--scenario <name>`, the `fixtures::scenarios`
registry (each scene naming its landmarks, so a camera can be aimed by
name) and `scripts/fake-grid.sh`, which is the "start the grid on a fixed
port" step below. Two of its lessons are the runner's too: a readiness
probe against a port proves the *port* answers, not that the grid you
started did — refuse a port something already answers on — and `kill -0`
says yes to an exited-but-unreaped child, so it is not a liveness check
for a viewer either.

[[viewer-screenshot-fixed-resolution]] and [[viewer-capture-layers]]
(2026-09-02) shipped the pinned capture size and the layer switches, and
between them settle what the runner passes each viewer. One environment
block does both sides: `SL_VIEWER_CAPTURE_SIZE` (default 1080p) and
`SL_VIEWER_CAPTURE_{UI,HUD,GIZMOS}` (default off, so a frame holds the
world alone). Neither viewer resizes its window for a world capture, and
the runner should not try to either — a window size is a request a tiling
compositor answers with its own, mid-run. The exception is a **UI**
comparison: Firestorm cannot draw its UI at a size other than the
window's, so a UI run wants a window that already matches the capture
size, and says so in its log when it does not get one.

The driver. Start `sl-fake-grid` on a fixed port with a named scenario,
launch this viewer and Firestorm against it with the same capture size,
captured layers, camera and day position, wait for both, and collect
`<run>/<viewer>/frame_NNN.png`, `scene.json` and `harness-status.json`.

Firestorm gained the matching surface in its own tree: `--credentials`
(the TOML file this workspace already uses), `--avatar`, `--gridfile`,
`--screenshot-dir`, `--camera-position`, `--camera-look-at`,
`--scene-dump`, plus `SL_VIEWER_SCREENSHOT_{DELAY,INTERVAL,FRAMES}`,
`SL_VIEWER_SKY_DAY_POSITION`, `SL_VIEWER_CAPTURE_SIZE` and
`SL_VIEWER_CAPTURE_{UI,HUD,GIZMOS}`. Three things
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

Done (2026-09-02): the `sl-crosscheck` crate and its binary. One grid, both
viewers in turn, and a run directory holding `run.json`, the two
configuration files, and per viewer `frame_NNN.png`, `scene.json` (when the
viewer writes one), `harness-status.json` and its own `viewer.log`.

Three decisions the plan did not settle, each with the reason it went the
way it did.

**The grid runs inside the runner** rather than as a spawned
`sl-fake-grid`. This is the same lesson as the launcher's port check, taken
one step further: a readiness probe proves a *port* answers, and binding
the port in-process makes "the grid that answered is not the grid you
started" impossible rather than detectable. `AddrInUse` is an immediate
error, and the grid is ready when `start()` returns.

**This viewer had to gain two things before it could be driven at all**,
neither of which was visible from the plan:

- `harness-status.json`. Firestorm already wrote one; we did not, so half
  the pair had no way to say whether its run happened. Ours is the same
  five keys (`sl-viewer-world-view/src/harness_status.rs`), written before
  the logout, and the failure paths that had none now have one: a login
  that never landed, a frame that never reached the disk, no camera to
  capture. **A run that never got in world used to write a full set of
  black frames and report them as a successful capture** — the schedule's
  delay was a timeout on *quiescence*, and nothing checked the region had
  ever come up. It now waits out `SL_VIEWER_LOGIN_TIMEOUT` (180 s, the
  variable and default Firestorm uses) and fails the run instead — but only
  for a run that logs in: `--replay` has no grid and never will, so the
  plugin carries a `grid_expected` flag rather than making an offline
  avatar replay wait for a region that is not coming.
- A `SIGTERM` handler. Without one, the runner's only way to end a stuck
  run was `SIGKILL`, which strands the grid session and makes the *next*
  run fail to log in — the exact failure the plan warns about, and it would
  have been ours to hit. `SIGTERM` and `SIGINT` now route into the same
  graceful logout the Quit menu takes, so the escalation is ask → grace →
  kill, and `Ctrl-C` on a run watched from a terminal logs out too.

**A one-sided run is not a failed run.** The first version judged a run by
whether both viewers produced frames, so `--only sl-client` — a legitimate
thing to want, and the only thing possible on a machine with no Firestorm
build — exited non-zero. The exit status now follows *what was asked for*:
every viewer that was asked to run produced usable frames. The report
keeps three cases apart in words as well: a comparison, a one-sided run as
asked, and a run that did not happen. Nothing in it ever says the viewers
agree or differ, because nothing in the crate has looked at a pixel.

Smaller things worth knowing before the next run:

- Each viewer is confined to the run directory — `FIRESTORM_X64_USER_DIR`
  for Firestorm, all four `XDG_*` roots for ours. Not only the cache: this
  viewer rewrites its settings on the way out, so a harness run would
  otherwise edit the operator's own.
- `BEVY_ASSET_ROOT` is passed to our viewer, because Bevy resolves its
  asset root from the *executable* and a `target/release` build finds no
  skins beside it. Invisible until a run captures the UI layer.
- The camera is aimed at a **landmark by name** (`--look-at mesh-cube`),
  from `--look-from` metres south and `--look-above` metres up. South
  because the fixture rows run west to east, so a camera to the south sees
  the row rather than the end of it.
- The machine-specific part of a run — where this machine keeps its
  Firestorm build — is `SL_CROSSCHECK_FIRESTORM` in an uncommitted `.env`,
  read before the arguments are parsed. A `.env` line that will not parse
  is reported **without the line**: it is where a person keeps what they
  did not want in this repository.
- Verified end to end against the catalogue scene, **both viewers**: each
  logged in as its own session in turn, captured its frames at the aimed
  camera, wrote its status and logged out cleanly (ours 55 s, Firestorm
  39 s). Both frame sets are 1920×1080; Firestorm also wrote its
  `scene.json`, and ours writes none yet ([[viewer-scene-dump]]).
- The first pair of frames already found something, which is the argument
  for the whole tool: with both cameras at the same pose — Firestorm's dump
  confirms the exact numbers the runner asked for — the two viewers frame
  different amounts of the row, because our vertical field of view is 45°
  and the reference's is 60°. That is a fidelity bug of ours rather than a
  harness one, and it is [[viewer-camera-fov-parity]].
