---
id: test-e2e-stage
title: sl-e2e Stage — a fake grid, several viewers and grid control in one test
topic: test
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-driver, test-e2e-viewer-process-launch]
refs: [server-fake-grid-scripted-avatars, viewer-fake-grid-render-harness]
---

Context: [context/automation.md](../context/automation.md),
[context/testing.md](../context/testing.md).

The piece a test starts with: bring up a grid and as many viewers as the
scenario needs, hand the test handles to all of them, and tear everything
down cleanly whatever happens. **Nothing here may assume one viewer.**

## Wanted

- `Stage` in a new `sl-e2e` crate: one in-process `sl-fake-grid`
  (`FakeGridBuilder` — regions, accounts, scenario, `deterministic(seed)`),
  N viewers, each logged in as its own account, and the grid-control
  handle (`FakeGrid`, `FakeAgent`, timeline, `mark` / `wait_marker`).
- Backend per viewer from `SL_E2E_BACKEND=process|in-process|both`
  (`both` runs the test once per backend); the process backend launches the
  real binary `--headless --automation-socket` through
  [[test-e2e-viewer-process-launch]].
- A slot for scripted avatars once [[server-fake-grid-scripted-avatars]]
  exists (the `sl-client-tokio` session it defines, not a new client).
- Per-test artifact directory under `target/e2e/<test>/<viewer>/`: viewer
  log, driver failure artifacts, the grid's event log.
- Tests live in `sl-client-bevy-viewer/tests/` so cargo builds the binary
  and exposes it as `CARGO_BIN_EXE_sl-client-bevy-viewer`.
- A machine with no GPU adapter skips loudly, as the full-stack tier does.
- A nextest `e2e` test-group beside `gpu` in `.config/nextest.toml`, with a
  bounded thread count and its own slow-timeout.
- `context/testing.md` gains tier **E — end-to-end** with the tier rule
  applied: a test belongs here only if nothing lower can produce its
  failure.

Acceptance: a two-viewer test on both backends logs both in, has one open a
floater while the other waits for a marker, and tears down with no stranded
sessions and no leftover processes, even when the test body panics.

## Outcome (2026-09-30)

- **`sl-e2e`** (`StageBuilder` / `Stage`). `StageBuilder::new(test)
  .viewer_binary(..).viewer("Alpha").viewer("Beta").run(async |stage| ..)`
  runs the body once per backend on a fresh grid: the stock scene's region
  (or `.region(..)`), an account `Stage <label>` per viewer, and anything else
  through `.configure_grid(|builder| ..)` (scenario, timeline,
  `deterministic(seed)`, gates). Every viewer has logged in and is quiet
  before the body starts. The body gets a driver `Viewer` per label, the
  `FakeGrid` (`grid()`), a viewer's session in its current region
  (`agent(label)`), and markers (`mark(label, name)`,
  `wait_marker(label, name, timeout)`).
- **Backends**: `SL_E2E_BACKEND=process|in-process|both`, unset meaning both,
  or `.backends([..])`. It applies to the whole stage, so every viewer of one
  run shares a backend. The process backend launches the binary `--headless
  --automation-socket --start uri:… --capture-size 1280x720
  --disable-web-media` through `sl-viewer-launch`. The in-process backend
  builds the viewer's own `ViewerAppBuilder` App on an `InProcessHost`, with
  the same window size and its own `ViewerPaths` under its directory.
- **Teardown always runs**. A panicking body is caught, the stage is taken
  down, and the panic is resumed; if the teardown failed too, the panic
  carries both. Teardown asks each viewer to log out: the `TerminationFlag`
  in process, `stop_all` for processes. It fails on a viewer that did not
  quit when asked, and on any session still in any region.
- **Artifacts**: `<target>/e2e/<test>/<backend>/`. The backend level is added
  to the planned path because both backends run in one test. It holds
  `grid.log` (every line outside a viewer span: the grid and the test) and,
  per viewer, `viewer.log`, `failures/` (the driver's artifacts) and `state/`.
  An in-process viewer's lines are filed by a tracing layer keyed on its
  `viewer{name=<test>/<backend>/<label>}` span. The fake grid has no
  structured event log of its own, so `grid.log` is its tracing output.
- **Markers**: `GenericMessage` gained a `Debug` that prints UTF-8 parameters
  as strings (method, params, invoice), and the driver gained
  `Viewer::events_from_start` and `EventCursor::wait_for_containing`. A marker
  wait is a logged-entry wait for `method: "sl-fake-grid-marker", params:
  ["<name>"]` from sequence number 0.
- **No GPU adapter**: a wgpu adapter probe before any grid starts; the stage
  logs a warning and returns `Ok`.
- **Scripted avatars**: there is no slot yet. It lands with
  [[server-fake-grid-scripted-avatars]], which does not exist, so there is no
  forward-looking API for it.
- The teeth test found a bug in `sl-viewer-launch`: a viewer that died of
  another signal after the `SIGTERM` (here a `SIGKILL` landing late, in
  general a crash during the logout) was reported as `AskedToQuit`. Only an
  exit, or death by the `SIGTERM` itself, counts now. The first hook run
  after that change caught exactly such a crash. A viewer process logged out
  cleanly and then died of `SIGSEGV` at exit, because Bevy's pipeline
  compiles, still running on task-pool threads, outlived the App while the
  Vulkan validation layer's exit handler tore down their state. The Bevy fork
  (`de4567d1`) makes a dropped `PipelineCache` cancel and await every
  in-flight compile.
- Acceptance: `sl-client-bevy-viewer/tests/e2e_stage.rs`, in nextest's `e2e`
  group (`binary(/^e2e_/)`, one at a time, 180 s slow period).
  - On both backends, Alpha opens its inventory by the toolbar button while
    Beta waits for a marker the test sends once Alpha's window is up, and
    Beta's inventory stays shut.
  - A panicking body on each backend comes back as its own panic, not a
    teardown failure, and leaves no viewer process.
  - A `SIGKILL`ed process viewer fails the teardown (`NoLogout`), and the
    stranded session is logged in `grid.log`.
  - About 21 s for all three in a release build.
