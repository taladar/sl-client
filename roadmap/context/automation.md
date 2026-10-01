# Context — viewer automation (the end-to-end driver)

Non-task prose for the `viewer-automation-*` and `test-e2e-*` tasks: a
mechanism, modelled on browser test frameworks (Playwright, Selenium), that
drives the **real viewer** and reads its state back with no human in the
loop. Read [context/testing.md](testing.md) first — this adds a tier on top
of the ones described there, it does not replace them.

## Why

Every tier below this one cuts the app somewhere. `InteractionTest` and
`WorldTest` drive *fragments* of the viewer and address widgets by `Name`
string; `ViewerHarness` runs the whole viewer (`ViewerAppBuilder`) against the
fake grid but steps it by hand and never clicks; `sl-crosscheck` runs the real
binary but can only pass it flags and read files it wrote. Nothing can open a
floater in the real binary, click a disabled button and see that nothing
happened, or have two logged-in viewers watch each other. So the last step of
verifying a feature is still a person logging in and clicking. This tier is what
replaces that person.

## Requirements (user, 2026-09-28)

- Find UI elements and in-world objects by **semantic** queries, not
  coordinates; drive real input; read text and text state (disabled,
  checked, selected, expanded, focused, read-only, values).
- **One test drives several viewers and the fake grid's regions.** Nothing
  in the design may assume a single viewer or a single avatar: viewers are
  addressed by handle, per-viewer state is per App, artifacts are per
  viewer.
- **Both transports from the start**: the real binary out of process, and
  viewer Apps in the test process — one API over both, so a test chooses the
  backend by environment, not by rewriting.
- **Windowless by default**: a test viewer opens no OS window, needs no
  compositor, and ignores the real mouse and keyboard; `--watch` opens a
  window for a human to look at.
- The same channel serves **CLI / agent driving** of a running viewer, runs
  against **live grids** (local OpenSim, aditi), **feeds AccessKit** (the
  semantic model *is* the accessibility model), and leaves room for the
  patched **Firestorm** to answer the same locators
  ([[test-crosscheck-ui-scenes]]).
- The fake grid becoming a full grid is a **separate, orthogonal track**. No
  task here is blocked on a fake-grid feature; a test that needs one names
  the server task in `refs` and runs on a live grid meanwhile.

## Shape

```text
 test (cargo/nextest)          sl-viewer-ctl (CLI / agent)
        │                               │
   sl-e2e  Stage ──────── sl-viewer-driver (async API, auto-wait, expect,
   │  fake grid (in-proc)        │           failure artifacts)
   │  N viewers  ────────────────┤
   │  scripted avatars           ├─ in-process transport ─► ViewerAppBuilder App
   │                             └─ remote transport (JSON-RPC, Unix socket)
   │                                        ▼
   └──────────────────────────► viewer process (--automation-socket, --headless)
                                  sl-viewer-automation plugin:
                                   semantic UI model ─► locator engine
                                   world model  ·  state probes
                                   synthetic input  ·  request executor
                                   (same model ─► AccessKit nodes)
 sl-automation-proto: Locator / WorldLocator / Request / Response / Node types
```

| Crate | Role | Task |
| --- | --- | --- |
| `sl-automation-proto` | pure serde vocabulary, viewer-neutral | [[viewer-automation-protocol]] |
| `sl-viewer-automation` | in-viewer plugin: model, locators, executor (input: `sl-viewer-ui-core`'s `synthetic_input`) | [[viewer-automation-executor]] and its blockers |
| `sl-viewer-driver` | async client API over both transports | [[viewer-automation-driver]] |
| `sl-viewer-launch` | viewer processes: confinement, graceful stop | [[test-e2e-viewer-process-launch]] |
| `sl-e2e` | `Stage`: fake grid + N viewers + grid control | [[test-e2e-stage]] |
| `sl-viewer-ctl` | CLI over the driver | [[viewer-automation-ctl-cli]] |

## Rules

- **Semantic, not positional.** A locator names a role plus an accessible
  name — preferably the **Fluent key** (`name_key`), which is
  locale-independent — or a test id (the entity's `Name`), or visible text;
  scoped with `within` (a floater, a panel, a list row). Coordinates never
  appear in a test.
- **Strict.** An action on a locator that matches more than one node is an
  error listing the candidates, never "the first one".
- **Actionability before every action**, polled in the viewer each frame:
  attached, visible (not `Display::None`, not clipped out of its scroll
  area, inside the viewport), **stable** (bounds unchanged for N frames),
  **enabled** (no `InteractionDisabled` on it or an ancestor), and
  **receives events** (a UI hit test at the aim point lands on it or a
  descendant — i.e. nothing covers it). Scroll-into-view is part of
  actionability for scroll areas and virtual lists.
- **Input goes through the real input path**, never by triggering widget
  observers: the typed `bevy_input` messages plus their `WindowEvent`
  wrappers, one step per frame, written by the injector in
  `sl_viewer_ui_core::synthetic_input` (a queue of `InputAction`s that the
  running app drains itself, which the testkit's `interact` functions wrap). A
  test that asserts "disabled buttons do nothing" must be able to fail.
- **World clicks are pick-verified.** An object's aim point is a projected
  point the viewer's own pick resolver confirms hits *that* object; if no
  such point is on screen, "reveal" frames it with the camera first.
- **Waits live in the viewer.** A wait is a predicate over the model,
  evaluated each frame with a frame and a wall deadline; event streams are
  sequence-numbered logs read by cursor, so a slow reader never misses an
  event. No test sleeps.
- **Snapshots on request.** The semantic model is computed when asked, never
  every frame, so an automation-enabled viewer costs nothing while idle.
- **A failure explains itself.** Every timeout or not-actionable error
  carries the candidates, a semantic-tree excerpt around the scope, the
  event tail, and — from the driver — a screenshot, all saved to the test's
  artifact directory.
- **Off by default, and a runtime switch.** The remote endpoint exists only
  when asked (`--automation-socket`), as a `0600` Unix socket. Never a Cargo
  feature (it would double the `cargo hack` powerset).
- **The protocol grows with its consumers.** No forward-looking variants:
  each request, locator field or error kind lands with the task that first
  produces and consumes it.

## Known feasibility notes

- **Transport: our own, not `bevy_remote`.** BRP was the first candidate and
  was rejected on review (2026-09-28): its default plugin always registers
  world-writing methods on an unauthenticated TCP port, its render sub-app
  listener binds a fixed port (two viewers collide) and loses bind errors,
  and a port-0 bind never reports the real port. See
  [[viewer-automation-remote-transport]].
- **Rendering and picking without a window: an off-screen window**
  (decided 2026-09-28, [[viewer-automation-offscreen-window-spike]]). The
  problem: `bevy_ui` and `bevy_picking` only hit cameras whose render target
  equals the pointer's, so UI cameras rendering into an `Image` get no hits
  from a window pointer; about 25 viewer files read
  `Window::cursor_position()`; and a `Window` with no raw handle rendered
  nothing. That is why `InteractionTest` picks but never renders and the
  screenshot harness renders but never clicks.
  - **Chosen: `bevy::window::OffscreenWindow`**, a marker in the Bevy fork. A
    `Window` carrying it gets no platform window from `bevy_winit`, and
    `bevy_render` gives it an `Rgba8UnormSrgb` texture of its physical size as
    its swap chain (kept across frames, never presented, no surface
    configured); `Screenshot::primary_window()` reads it back. Every camera
    keeps `WindowRef::Primary`, so picking, the UI and the cursor readers are
    untouched — nothing in the viewer changes but who spawns the window. Pin
    its scale factor to 1 so a logical pixel (what the UI, picking and the
    injector speak) is a pixel of the frame.
  - **Rejected: custom pointers on an image** — a `PointerId` targeting the
    image fixes `bevy_picking`, but every `PrimaryWindow` cursor reader (GPU
    pick, camera, pie menus, floater and inventory drags, edit tools,
    tooltips…) would have to learn to ask "the pointer" instead, and each one
    missed is a feature that silently does nothing under automation.
  - **Rejected: an invisible winit window** — still needs a compositor, and on
    Wayland a compositor may not map, size or render an unmapped window.
  - **Prototype**: the full-stack test
    `an_offscreen_window_renders_the_ui_and_takes_clicks_and_picks`
    (the harness's `HarnessOptions::in_offscreen_window`): a fake-grid login
    rendered into a 1280×720 off-screen window; a synthetic right-click over
    the stock box resolves through the GPU ID-buffer pick to that box and
    opens its pie; the locator engine judges the toolbar's Inventory button
    actionable (its hit test is `bevy_picking` against the window's UI camera)
    and a synthetic click opens the floater, which the next frame draws.
  - The window's size is exactly its `resolution`: nothing resizes an
    off-screen window, which is also what a capture wants.
  - **Built** ([[viewer-automation-windowless-mode]]):
    `WindowMode::Headless { size, watch }` in `ViewerAppBuilder` (the
    binary's `--headless`, sized by `--capture-size`) spawns that window,
    installs `SyntheticInputPlugin`, drops the device plugins and gilrs, gives
    the App a private clipboard and runs a fixed 60 Hz `ScheduleRunnerPlugin`;
    the full-stack harness's `in_offscreen_window` builds through it. A
    headless `--screenshot-dir` run captures the window itself
    (`CaptureTarget::Window`) and hides the layers it was not asked for,
    rather than retargeting cameras into an image.
  - **`--watch`** keeps winit for a second window that is **not** primary
    and carries the fork's `ViewOnlyWindow` (winit drops its input events,
    and device mouse motion while no window takes input). It shows the frame
    preview quad; the render world copies the off-screen texture into the
    preview's image after each frame (`watch_window.rs`).
  - **A `Window` reader filters on `PrimaryWindow`.** With the watch window
    there are two windows: an unfiltered `Query<&Window>` either fails its
    `single()` or reads the watch window's (dead) cursor.
- Headless input isolation must also drop the device plugins (evdev,
  SpaceNavigator) and gamepads, not only window events; the OS clipboard is
  replaced by a per-App one so copy/paste is testable and never touches the
  user's. The viewer has **one** clipboard, Bevy's `Clipboard` (built with
  `system_clipboard` in the viewer); a non-windowed App inserts the fork's
  `Clipboard::in_process()` before `DefaultPlugins`, and a plugin that may
  stand alone falls back to `sl_viewer_platform::clipboard::
  init_private_clipboard`, never to a default (OS) one.
- AccessKit needs a winit window, so it is absent headless; the semantic
  model must never depend on it.
- **Two viewers in one process are two Apps with two roots**
  ([[viewer-automation-per-app-state]]). Every on-disk location is read from
  the App's `ViewerPaths` resource (`sl-viewer-platform/src/paths.rs`), which
  the builder inserts before any plugin —
  `Storage::Directories(ViewerPaths::under(root))` gives a viewer a tree of
  its own (`config/`, `state/`, `cache/`, `snapshots/`), `Storage::Ephemeral`
  nothing on disk, and a World with no `ViewerPaths` keeps nothing on disk
  either (a unit test's stores run in memory). The render knobs are
  `RenderOverrides` fields, the termination flag is a `TerminationFlag`
  resource (the process's signal flag unless the App is given
  `TerminationFlag::own()`), and the settings store is per App.
  `per_app_test` logs two Apps into one fake grid and holds each to its own
  agent, directories, settings, overrides, log lines and logout.
- **What stays process-wide**, deliberately: the tracing subscriber, Bevy's
  task pools, the shared tokio runtime and HTTP pool (and the one HTTP proxy),
  the static-asset library, the web-media profile (Chromium starts once per
  process), the signal handler. A `SL_VIEWER_LOG_*` switch and a debug dump
  destination (`SL_VIEWER_DUMP_DIR`, `_DUMP_MEDIA_FRAMES`, `_CAMERA_DUMP`) stay
  environment reads: what they change is diagnostic output, not the viewer.
- **No system reads the environment.** A behavioural knob is read once while
  the App is built: into `RenderOverrides` / `AvatarOverrides` (stated by a
  test through `ViewerAppOptions::{render,avatar}_overrides`), into the
  camera start (`OrbitSeed`), or by the plugin that registers the debug
  system (a floater or preferences tab to open, a demo, a focus framing).
- **Log lines are told apart by span.** `ViewerAppOptions::log_label` builds
  and updates the App inside a `viewer{name}` span (`ViewerApp::update`,
  `ViewerApp::finish`). The Bevy fork's `bevy_tasks` runs every task in the
  tracing context it was spawned from (subscriber and span), and
  `sl_client_bevy::log_context` does the same for the session's threads and
  shared-runtime tasks, so a line logged on any of them carries the span —
  and reaches a test's scoped subscriber. A `trace`-feature (profiling) build
  gives system spans `parent: None`, which hides the `viewer` span from those
  lines' scope.
- **Each App opens its own wgpu device.** Windowless on the stock fake-grid
  region (2026-09-29): the first App adds ~195 MiB resident at build (mostly
  one-time process cost), the second ~53 MiB; both logged in, ~1.3 GiB.
- **A virtual list is strict only over its bound rows.** The locator engine
  pages a list when nothing matches, but it does not page to the end to
  prove a bound match unique — about three frames a page, 50 s on a
  10 000-item inventory. A browser driver has the same blind spot with a
  virtualised table.
- **Object names are not streamed.** `ObjectUpdate` places an object;
  its name, description and owner come only in a property reply. The world
  model keeps every reply it sees (`ObjectFacts`, filled by
  `WorldModelPlugin`). A `WorldQuery` that compares a name or an owner asks
  for the missing ones with `RequestObjectPropertiesFamily`, which is the
  hover's request and selects nothing. `RequestObjectProperties` would
  select the object on the grid.
- **World positions are region-local to the agent's region.** An avatar is
  placed by its tracked object entity, not by its anchor: the anchor (a
  sphere or a body root) does not carry the region basis.
- **A world action aims through the viewer's own pick**
  ([[viewer-automation-world-aim]]). `WorldAim` probes candidate points on
  the target's box through `PickProbes` (`sl-viewer-world-api`), which the
  GPU ID-buffer pick answers live and the CPU ray-cast double answers in the
  fixture world — the same resolver every real click goes through, so an aim
  point is one a click lands on. A pick submission renders one pixel, so
  `GpuPicker::take_requests` serves one pixel's requests a frame and leaves
  the rest queued; never fold requests at different pixels into one pick.
  A target no point reaches is framed once with `FrameObject` (the camera
  keeps colliding, except with the framed object's own prims) and the camera
  stays where that left it. After that reveal a target still off screen or
  covered is looked at again until the deadline (`WorldTimedOut` names the
  check): the camera eases into a framing, and the last stretch of the glide
  is slow enough to pass the stability rule.
- **State probes read models, and some models live in heavy crates**
  ([[viewer-automation-state-probes]]). `sl-viewer-automation` reads the
  light ones itself (identity, region entity, parcel, `CameraMode`,
  `SelectionSet`, `InventoryModel`, `NotificationManager`); the
  conversations (`sl-viewer-people`), the live toasts (`sl-viewer-notices`),
  the teleport overlay (`sl-viewer-places`), the balance (the status bar)
  and the scene's outstanding work (`SceneQuiescence`, world-view) come
  through `ProbeSources` — plain `fn(&mut World)` pointers the viewer's
  assembly registers (`sl-client-bevy-viewer/src/automation_sources.rs`),
  run only when asked. The owning crate exports the reader; the automation
  crate never depends on it.
- **Every toast card declares its buttons** (`ToastButtons`, in
  `sl-viewer-notifications`): the catalogue host from the template form, and
  every bespoke card through `ToastSpec::buttons` — a script dialog's own
  buttons, a permission request's, an offer's. A notification no longer shown
  reports its template's form.
- **A heard chat line has no channel.** `ChatFromSimulator` carries none: a
  viewer hears public chat and the script-only kinds (owner-say, region-say
  to it, the debug channel), which the readout's `ChatKind` names. The
  channel of what the own agent *said* is in its `Chat` command, in the event
  log.
- **The event log records in `Last`**, one counter over session events,
  outbound commands and UI actions, clones kept and printed only when read.
  The diagnostics tally is a `tracing` layer: the binary's `init_tracing`
  feeds `LogTally::global()`, so like the subscriber it is process-wide; an
  in-process test names its own with `DiagnosticsSource` and a scoped
  subscriber.
- **Build mode is judged by the build tool's own resolvers**, not the world
  pick: a press by the selection gesture's `ObjectPicker` (a handle the rig
  takes first), a rubber band by its `sweep_candidates`, a handle drag by
  the rig's hit test and drag math — asked through `SelectionProbes` and
  `ManipulatorProbes` (`ProbeQueue`s in `sl-viewer-world-api`). A handle drag
  names its snap regime: the grid engages only past the snap guide, so where
  the pointer ends decides whether the amount is exact or lands on the grid.
- **A world aim's stability is judged on screen**: the target's box corners
  projected, each within 5 % of the projected box's smaller side of the last
  poll (never under half a pixel), the target within a centimetre — never
  the camera eye. The follow camera holds the own avatar's animated head,
  which sways a couple of centimetres with the idle animation; the old
  0.1 mm eye tolerance restarted every multi-frame GPU probe, so no world
  action on a live, freshly logged-in viewer ever landed, and a fixed
  half-pixel tolerance still did under load (at ~10 fps the sway is ~0.6 px
  a poll; found by the driver acceptance in the commit hook's parallel run,
  2026-09-29). Candidate points sit a fifth of a face from its edges, so 5 %
  keeps a probed point on the target. The sweep and handle drags'
  `CameraStill` still compares the eye: it only gates a two-poll streak and
  revalidates nothing afterwards.
- **Rezzing is a world action on what the rez lands on** (`WorldAction::Place`,
  the driver's `place()`): a click with the Create tool, waiting for that tool
  (`ActionabilityCheck::CreateTool`) and aimed through the pick resolver like a
  touch. There is no locator for bare ground, so a test rezzes on an object.
  A **select or sweep waits for a tool that selects**
  (`EditTool::selects_objects`): the Build window opens on the Create tool
  when nothing is selected, and a click there rezzes. A plain Create click
  selects nothing (the reference's `LLToolCompCreate`); only the rez's
  drop-into-edit selects, and only an object that arrived after the rez.
- **What a viewer may do with an object is per viewer on the fake grid too**:
  every `ObjectUpdate` leaves through `world::send_objects`, which stamps the
  receiving agent's `OBJECT_*` permission flags from the object's owner and
  masks (OpenSim's `GenerateClientFlags`). A new prim is its owner's alone. A
  test that needs another viewer to edit an object sets its everyone mask.
- **Status bar read-outs are addressable**: `status-readout:region`,
  `:coordinates`, `:parcel`, `:balance`, `:time`, `:fps` — what the bar shows,
  as against the `Status` probe, which reads the models it is drawn from.
- **The environment probe reads the sky RLV reads**: `Probe::Environment` is
  `RlvEnvironmentSlot` (`sl-viewer-world-api`) — the sky the scene publishes
  for `@getenv_*`, which is the one it renders, and `fixed_sky`, whether the
  local layer (a menu preset or a script's `@setenv_*`) stands in for the
  shared sky. The ambient is in the settings' own units, not `@getenv`'s. The
  agent readout's `heading` is `AvatarControls::held_heading`, the viewer's
  own heading (counter-clockwise from east), not the simulator's echo.
- **The RLVa console is the RLV tier's driver** (`tests/e2e_rlv.rs`): a line
  typed there runs as an object's `@`-command with the agent as issuer, and
  closing the console lifts what it holds. The RLVa windows open over the
  console's input, so a test issues its commands first and opens the windows
  after. Turning RLV on raises the `RLVaToggledOn` toast over the top right
  of the screen (floater close buttons included); answer it first.
- **The pilot suite** (`tests/e2e_pilot.rs`) is the worked example of each
  kind of test: chrome, two viewers on one object, a pie and a touch the grid
  sees (a test prim put on the grid through `FakeAgent::with_world`), and a
  teleport through the world map between two stage regions.
- **The viewer has no grab-drag of an object outside build mode yet**, so
  there is no aimed grab; it lands with the Move tool
  ([[viewer-build-tool-row-parity]]).
- **The executor** ([[viewer-automation-executor]]) is `AutomationPlugin`
  (`sl-viewer-automation/src/executor.rs`): requests go into the
  `AutomationQueue` resource and responses come out of it, so a transport is
  whatever moves them between it and a test. It runs as one exclusive system
  in `Last`, after the event log records, and costs a resource check a frame
  while nothing is submitted. Reads and waits run side by side; the requests
  that play input take turns in submission order (one pointer). The viewer
  installs it with `ViewerAppOptions::automation` (the full-stack harness
  does), never by default.
- **Every error response carries a `FailureReport`**: the tree excerpt
  around a UI locator's scope (the scope's subtree, else the top of the
  tree; depth- and node-capped), the event log's last 32 entries, and the
  warnings and errors logged since the request started
  (`diagnostics_cursor`). The error itself carries the check, the
  candidates and the last observation.
- **The remote transport** ([[viewer-automation-remote-transport]]) is
  `RemoteEndpoint` + `RemoteAutomationPlugin`
  (`sl-viewer-automation/src/remote.rs`), installed by
  `Automation::Socket(path)` (`--automation-socket [PATH]`). One line of JSON
  per `Request`; the viewer writes `ViewerMessage`s. It renumbers requests
  from `REMOTE_ID_BASE` (anything else submitting to the same queue stays
  below it) and gives answers back under the client's id. A socket path is
  limited to about 100 bytes, and the bind stages in `<dir>/.<pid>.<n>/s`, so
  a deep directory fails with "shorter than SUN_LEN" — the default under
  `/run/user/<uid>` is short. Requests that play input need `--headless` (the
  synthetic input is installed only there).
- **The in-process transport** ([[viewer-automation-inprocess-transport]]) is
  `InProcessTransport` (`sl-viewer-automation/src/in_process.rs`): it hosts
  `HostedApp`s (a plain `App`, or the viewer's `ViewerApp`, stepped in its
  span), submits straight to each one's queue, and steps every live App a
  frame per round (2 ms apart) while a caller waits, so a wait on one viewer
  never pauses another. Both transports keep their clients through one
  `Relay` (`relay.rs`): ids renumbered from a base (`IN_PROCESS_ID_BASE` 2⁴⁷,
  `REMOTE_ID_BASE` 2⁴⁸), duplicates refused, answers delivered in the order
  the executor gave them. A Bevy `App` is not `Send`: the Apps are built and
  stepped on one thread, the caller's.
- **The in-process host** (`InProcessHost`,
  `sl-viewer-automation/src/in_process_host.rs`) runs an
  `InProcessTransport` on a thread of its own and steps every viewer it hosts
  continuously, 2 ms apart, as a process runs — waiting or not. Apps are
  built there (a closure hands the host the builder), a test reaches into one
  between frames with `with_app`, and each viewer is reached through a
  `ViewerLink`: a request channel and a message channel, the shape a socket
  connection has. An exited viewer closes its link; `stop` joins the thread
  after the pipelines finish.
- **The driver** (`sl-viewer-driver`) depends on the protocol and tokio
  only, not on Bevy or the viewer. Its transport boundary is that channel
  pair, not a trait: `Viewer::connect` bridges a socket to one,
  `Viewer::over_link` takes the host's. The connection numbers requests from
  1, routes answers by id and subscription pages by subscription, and fails
  whatever waits when the viewer's side closes. A request's deadline is
  **wall-clock only** (`frames: u32::MAX`): an in-process viewer is stepped
  as fast as it renders, so a frame count would expire early. Past the
  deadline the driver gives the viewer a grace (30 s) before `NoAnswer`.
- **Reads are strict in the viewer**: a `UiLocator` read waits for
  `Attached`, then asks `Snapshot { within: locator }`, which resolves the
  locator strictly — an ambiguity comes back as the viewer's own
  `Ambiguous` with its report, not a driver-side guess.
- **Failure artifacts** go to `<artifact_dir>/<NNN>-<action>/`:
  `screenshot.png` (the locator's matches outlined; without the outline when
  its scope does not resolve), `tree.txt` (the report's excerpt, else the top
  three levels of the whole tree) and `events.txt` (the report's event tail
  and diagnostics, else the log's last 32 entries). Each one that cannot be
  saved says why in the error instead; a viewer handle without an artifact
  directory saves nothing and says so.
- **Two viewers of one test body** (`automation_driver.rs`): both are Apps on
  one host, one reached over its link and one over its automation socket —
  the socket transport is the one a viewer process serves, so the process
  backend differs only in who launches the viewer
  ([[test-e2e-viewer-process-launch]]).
- **A viewer process is launched and stopped through `sl-viewer-launch`**
  ([[test-e2e-viewer-process-launch]]), shared with `sl-crosscheck`: a
  `ViewerDir` per viewer (the four `XDG_*` roots under its `state/`, its
  `viewer.log`), `Launch::in_dir` plus the flags (`--headless`,
  `--automation-socket`), and `RunningViewer`, stopped by `SIGTERM` → logout
  grace → `SIGKILL`. `stop_all` stops several in parallel, and dropping a
  running one stops it the same way, so a panicking test logs its viewers
  out. The viewer turns `SIGTERM` into a logout at any point after start-up
  (`tests/viewer_processes.rs` checks the grid sees `LoggedOut` for both).
- **The stage** ([[test-e2e-stage]]) is `sl-e2e`'s `StageBuilder::run`: a
  fresh fake grid per backend, an account `Stage <label>` per viewer, each
  viewer logged in and quiet before the body, and a teardown that always
  runs (a panicking body is resumed only after it) and fails on a viewer
  that would not log out or a session left on the grid. It depends on the
  viewer's library and the viewer's integration tests dev-depend on it: a
  dev-dependency cycle Cargo allows, so the stage is for `tests/`, never for
  the crate's unit tests. An in-process viewer's `log_label` is
  `<test>/<backend>/<label>`, and a process-wide tracing layer (`logs.rs`)
  files every line in that span into the viewer's `viewer.log`; lines
  outside any viewer span go to `grid.log`.
- **A stage runs on a live grid by environment**
  ([[test-e2e-live-grids]]): `SL_E2E_GRID=fake|opensim|aditi`, unset being
  the fake grid, so the commit hook never logs into a live one. On a live grid
  there is no `FakeGrid`: `Stage::grid`, `agent`, `mark` and `wait_marker`
  answer `NoGridControl`, the accounts come from the grid's credentials file
  in `SL_E2E_AVATARS` order (default `primary`, `secondary`, `tertiary`, then
  the rest), and the home region and agent ids are read from the agent probe
  once each viewer has arrived. A test declares what it needs
  (`StageBuilder::needs`: `Need::GridControl`, `Need::Content`; `region`,
  `configure_grid` and `start_position` imply `Need::DictatedGrid`), and the
  stage skips a grid that cannot provide it — or has fewer accounts than
  viewers — with a warning naming why, before it reads a credentials file or
  logs anything in. The default live start is OpenSim's `Default Region`
  centre (conformance's, and one spot for every viewer, so they can hear each
  other) and aditi's `last`; `SL_E2E_START` overrides it. Who a viewer is
  heard as is `Stage::account_name`, never a hard-coded `Stage <label>`. A
  live viewer gets ten minutes to settle (`LIVE_SETTLE`: aditi's asset service
  answers some textures 503 for minutes, each walking its retry chain), and a
  live run goes under nextest's `live` profile, which never kills a test — a
  killed test strands its avatars on the grid.
- **One login cooldown for every unattended harness**: `sl_repl::LoginCooldown`
  keeps a per-avatar stamp under `$XDG_STATE_HOME/sl-client/login-cooldown/`,
  shared by `sl-conformance` (which refuses) and the stage (which waits the
  window out) and by every worktree. A stage on aditi therefore waits two
  minutes between its two backends.
- **A wait for quiet names what is not**: the `Quiescence` readout carries
  `outstanding_by`, the outstanding work bucket by bucket (`<store>.<stage>`
  such as `meshes.downloading`, `textures.deferred`, or a build queue's name),
  empty buckets left out — read from `SceneQuiescence::breakdown`. The first
  aditi run found the mesh store's skin and physics loads leaving their
  entries at `Downloading` for good (63 on a busy region), which only the
  breakdown could tell from a slow region.
- **An aditi second factor in process**: the App ends on the MFA challenge
  (`LoginOutcome`), which the stage reads from the exited App, answers with
  the avatar's `mfa_command` and logs in again with a new App — the loop the
  binary's `run_viewer` runs. A process viewer answers it itself.
- **A marker is found in the event log by its printed detail**:
  `GenericMessage`'s `Debug` prints UTF-8 parameters as strings in the order
  method, params, invoice, so `Stage::wait_marker` waits for a
  `GenericMessage` entry containing `method: "sl-fake-grid-marker", params:
  ["<name>"]` from sequence number 0 (`Viewer::events_from_start`). A marker
  that arrived before the wait began still counts.
- **Event-log subscriptions live in the executor**, not the transport: a
  `subscribe` answers at once and its notifications queue beside the
  responses, so the in-process transport streams them the same way.
- **A screenshot travels as a file**, not bytes: the request names an
  absolute path the viewer writes the PNG to. The requester shares the
  machine (local socket or same process) and keeps it as an artifact anyway.
- **A state wait tests a probe's readout as JSON** at a JSON Pointer
  (`ValueTest`: present, absent, equals, `includes` — a structural subset, a
  string's substring). An inventory folder that is not there yet reads as
  `null`, so a wait can wait for it.
- **A fill** is a click into the field, `Ctrl+A`, `Backspace` and the text
  typed; it answers only once the field holds the text (`FillMismatch`
  otherwise). A fixture field needs a `TabIndex`, as every viewer field
  has: a click on an unfocusable field bubbles its focus request to the
  window, which clears it.
- **Selectors have a string grammar** ([[viewer-automation-ctl-cli]],
  `sl-automation-proto/src/selector.rs`): `Locator` and `WorldLocator` parse
  from and print as `window[test_id=floater:build] >>
  button[name_key=build-apply][enabled=false]` and
  `object[name=Door][near=own_avatar][radius=5][nth=0]` — a role or kind
  (or `*`), then `[field=value]` attributes named after the JSON fields,
  `~=` for a name's "contains", a value bare when it is plain
  (`[A-Za-z0-9_.:-]`) and quoted otherwise. `Display` *is* the grammar, so
  every failure message prints a locator a person can paste back; a test
  that asserts on an error's text asserts on it. A world selector is one
  step.
- **`sl-viewer-ctl`** ([[viewer-automation-ctl-cli]]) is the driver from a
  shell. `launch` (one viewer on a live or local grid, via its credentials
  file) and `stage <toml>` (a fresh fake grid plus N viewers, accounts
  `Stage <label>`) start real viewer processes `--headless` through
  `sl-viewer-launch`, wait until each has logged in and gone quiet, print
  each socket, and hold them until Ctrl-C / `SIGTERM`, then log them out.
  Their sockets go in the viewer's default socket directory
  (`$XDG_RUNTIME_DIR/sl-client-bevy-viewer/`), so a verb with no `--socket`
  finds the one viewer that answers there. Verbs (`tree`, `find`, `click`,
  `drag --onto|--by`, `fill`, `press`, `wait --for`, `open`, `menu`,
  `world find|touch`, `chat`, `notifications`, `agent`, `environment`,
  `file-dialog`, `screenshot`, `events --follow`; `press --hold N` holds a
  key N frames)
  are one driver call each; `attach` runs them line by line over one
  connection. It depends on the fake grid but not on the viewer's library, so
  it builds in seconds; it runs the release viewer beside itself. A `launch`
  onto aditi takes its turn under the shared login cooldown.
- **The cheap tiers speak locators** ([[viewer-automation-testkit-locators]]):
  `sl_viewer_automation::in_app` submits the protocol's requests to an
  `&mut App`'s own executor (installing `AutomationPlugin` if it is absent)
  and steps the app until each is answered, so a fixture test's click is the
  engine's — strict, actionable, played through the synthetic input — and its
  failure is the driver's own `DriverError::Failed`, printed `viewer <label>:
  <action> <locator> failed: <error>` as the end-to-end tier prints it, with
  the report in the error instead of an artifact directory. The waits count
  **frames** (`in_app::Options`, 600 by default), since a fixture is stepped by
  hand; `ViewerHarness` states a wall-clock deadline and has `click`, `hover`
  and `expect` of its own. The testkit crate itself cannot host them — the
  automation crate dev-depends on it, and a UI crate the automation crate
  depends on would get a second copy of itself in its own tests — so they
  are for the viewer crate and the crates above the automation crate.
- **A disabled control is proved inert with `click_while_disabled`**: a click
  never presses a disabled node (it waits for `enabled`), so the test that
  "disabled does nothing" waits for `disabled`, brings the pointer on as a
  hover does and presses there anyway. The first one written
  (`a_greyed_group_set_button_does_nothing`) found the Build floater's
  parameter tabs enabled with nothing ever selected: the gate redrew only on a
  changed snapshot, and "nothing selected" is `None` before and after.
- **The screen-reader tree is the model**
  ([[viewer-automation-accesskit-bridge]]): `AccessKitBridgePlugin`
  (`sl-viewer-automation/src/screen_reader.rs`, installed only for
  `WindowMode::Windowed`) switches off Bevy's own per-frame
  push (`ManageAccessibilityUpdates`) and sends AccessKit a tree built from the
  snapshot: the model's children are the tree's children (Bevy's own would hang
  a button off the window when its `ChildOf` parent has no `AccessibilityNode`),
  a hidden node is left out, a floater is a `Dialog`, text is a `Label` read as
  its value, an unnamed group a `GenericContainer`. Only what differs from what
  was last sent goes out; the model is read at most every
  `ACCESSKIT_REFRESH` (200 ms) and at once on a focus change, and only while
  `AccessibilityRequested` holds. An inactive adapter (the closure of
  `update_if_active` never ran) forgets what was sent, so the next listener
  gets the whole tree. A slider's range comes from the `AccessibilityNode`
  `bevy_ui_widgets` keeps on it.
- **The Linux adapter keys on `ScreenReaderEnabled`**, not `IsEnabled`
  (`accesskit_unix` watches only that property of `org.a11y.Status`). Without
  Orca, `busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status
  ScreenReaderEnabled b true` makes the viewer register, and the `Atspi` GI
  bindings read the tree a screen reader sees; set it back to `false` after.
- **What a stylesheet writes into `::before` / `::after` names nothing.** The
  skin draws icons that way (a glyph host's whole content, a tab arrow, a
  tick), and bevy_flair keeps a text node's two pseudo-element spans as its
  first and last children, so the model skips them. A control that shows only
  an icon takes a name of its own (`Semantic::name_key`,
  `menu::spawn_icon_menu_button`, `TabSpec::names`), and
  `automation_screen_reader::every_control_has_a_speakable_name` fails any
  control of any registered floater or element with no letter or digit in its
  name — **hidden ones included** (a control on an unopened tab or in a closed
  popup is named when it shows or never); only a hidden, unnamed list or tree
  row (a pooled row no item is bound to) is skipped.
- **A glyph button's name is part of its label's type**:
  `UiLabel::glyph(slot, name_key)` — there is no glyph label without one, so
  a glyph button on a panel no fixture ever builds (a conversation pane, a
  media bar) is named too.
- **Fields under one caption are named by their part**: a `NamePart(key)` on
  a field ("U", "Begin") makes the model call it `labelled-part = { $label }
  { $part }` from the caption its `LabelledBy` finds — "Offset U", "Path Cut
  Begin" — and a `SpokenLabel(key)` on a caption that abbreviates ("Path Cut
  (B/E)") gives the form that joins ("Path Cut"). The Build window derives
  both (`edit_tool::name_part` from the element's last segment,
  `spoken_label` by caption key). A harness's flat string table has no
  message with arguments, so there the pattern misses and the model joins the
  two in English order. `no_two_fields_of_a_floater_share_a_name` (every
  floater, shown fields) and `no_two_fields_a_tab_shows_share_a_name` (each
  Build tab opened in turn) guard it; fields on rows that swap by mode may
  reuse a caption, since they never show together.
- **A caption with `display: none` names nothing** in the model — a row that
  swaps its caption (the Build Object tab's Taper / Hole Size) can be
  `LabelledBy` itself and is called by whichever one shows.
- **The live-verify sweep** ([[test-e2e-live-verify-sweep]]) turned the
  features' "check it live" notes into tests; the remaining ones are its
  `test-e2e-sweep-*` tasks. The worked examples are
  `tests/e2e_live_checks.rs`: chords, a window's tabs and clipboard, a mode
  that holds offers, a list's sort and filter.
- **`UiLocator::press` clicks into the field first**, which collapses a
  selection: `Ctrl+A` then `Backspace` through a locator deletes one
  character. A chord that depends on the previous one goes through
  `Viewer::press` (whatever has the focus), and clearing a field is
  `fill("")`.
- **A virtual list's pooled rows count for strictness**: an unbound row is a
  hidden, unnamed `listitem`, so `window >> listitem` is ambiguous as soon as
  the list has spare rows. Name the row (`listitem[name~=Seated]`); `nth(0)`
  is the top bound row. The model names a parked row (`VirtualRow::index`
  `None`) nothing whatever text its binder left in it — most binders skip a
  parked row rather than blank it, and the debug-settings list filtered to one
  setting otherwise had two rows of that name.
- **The grid's side of an exchange is scripted through the viewer's
  session**: `stage.agent(label).with_sim(|sim| sim.send_instant_message(..))`
  delivers an IM or an offer from anybody, and what the viewer sends comes
  back on `agent.events()` decoded — an IM as `ServerEvent::InstantMessage`,
  an undo as `ObjectsUndone` — not as a raw `ClientMessage`. Where the fake
  grid does not yet do the result (an IM reaching another viewer, an undo
  moving the prim, two viewers seeing each other's avatars), a test asserts
  what reached the grid and names the server task.
- **A viewer that quits by itself** (a Quit chord) is announced first with
  `Stage::expect_quit`; the teardown then holds it to a clean exit instead of
  asking it to log out, and still fails a session left on the grid.
- **A second prim joins the selection by `shift_select`**
  (`WorldAction::ShiftSelect`, judged like a select, with `Shift` down a
  frame before the press). A link that lands leaves the new child in the
  selection; Link counts only selected linkset roots, as the reference does.
- **A relog is `Stage::relog(label)`** ([[test-e2e-sweep-relog]],
  `tests/e2e_relog.rs`): the viewer is asked to log out as the teardown asks
  it, must exit having done so and — on the fake grid — leave no session, then
  a new App or process starts on the **same** directories, logs in where the
  stage's viewers start (on aditi after its cooldown turn) and is waited for
  as at the start. It answers the new session's `Viewer`, and
  `Stage::viewer` answers that one from then on — so `Stage::viewer` hands
  out an owned handle (`let alpha = &stage.viewer("Alpha")?;` where a
  helper takes `&Viewer`), and a handle kept from before the relog talks to
  a viewer that is gone. A relogged process writes `viewer.<n>.log`.
- **A window is moved and resized by `drag_by`** (`RequestBody::DragBy`):
  press on the node, move by a relative offset, release — on a window's
  `floater-title-bar` or `floater-resize`. The offset is an amount, like a
  handle drag's, not a coordinate; where the window ends up is read back
  from its bounds.
- **A window opens wholly on screen** (`fit_on_open` in the floater
  manager, the reference's `adjustToFitScreen`): one taller than the room
  between the bars gives up content height down to its floor. At the
  stage's 1280×720, Preferences used to open with OK and Cancel under the
  bottom toolbar.
- **The fake grid's friends are a fixed fact**:
  `FakeGridBuilder::friends` puts each of two accounts in the other's
  login `buddy-list`, and every session can name every account
  (`GetDisplayNames`, and the legacy `UUIDNameRequest` from the same store —
  the friends list asks that way). Nothing announces presence; a test
  does, with the session's `send_online_notification` — again after a
  relog.
- **The grid's side of a notice or an offer is encoded in `sl-proto`**:
  `GroupNoticeReceived::instant_message` and `InventoryOffer::binary_bucket`
  are the inverses of the decoders, so a test never hand-builds a bucket.
- **A file chooser is answered by the test** ([[test-e2e-sweep-environment]]):
  a viewer with no window of its own (`Windowless`, `Headless`) puts no
  chooser on the desktop. `FileDialogBackend::Answered` (inserted by the
  assembly) holds an `OpenFileDialog` as the `PendingFileDialog`, with the
  gate shut, until `answer_pending_dialog` closes it with a path or Cancel —
  the same `FileDialogClosed`, the same remembered directory. The request is
  `AnswerFileDialog` (driver `answer_file_dialog`, `sl-viewer-ctl
  file-dialog PATH|--cancel`), which waits for the dialog, so it is sent right
  after the click that opens one. The automation crate reaches the platform
  crate's service through `ProbeSources::file_dialog`, since depending on it
  would bring the audio device and `rfd` into every automation build.
- **Every two-stage upload has its own uploader URL** on the fake grid
  (`…/upload/<ticket>`), so saves in flight together through one capability
  (a bulk import's) cannot overwrite each other's metadata.
- **A region can withhold capabilities**: `RegionConfig::withheld_caps` leaves
  the named `CAP_*` out of the seed grant, as a simulator without a feature
  does — how the settings-unsupported gate is tested.
- **The fake grid answers `CreateInventoryItem`** (`uploads::create_item`):
  a settings item starts as its kind's default asset with the subtype in its
  flags (OpenSim's `GetDefaultAsset`), a script as the starter script,
  anything else an empty asset or the transaction's upload; the reply is the
  `UpdateCreateInventoryItem` echoing the callback id. A UDP
  `MoveInventoryItem` (how a rename travels; the AIS3 move has no name) is
  applied to the serving tree by `SimSession` itself. A test reads what a save
  stored with `FakeAgent::stored_asset`.
- **The environment readout** carries the sky's haze density and sun
  (azimuth, elevation in radians) beside its ambient, the drawn water (name,
  fog density), `transition` — how far a manual change's cross-fade has got —
  and `previewing`, the editor windows previewing through the edit layer (by
  floater id, the one on top last); all but the sky come from the scene through
  `ProbeSources::environment_scene`. The sky in it is the target the fade runs
  toward, never the blend on screen.
- **Every environment control is swept below this tier**:
  `sl-viewer-environment`'s `preview_harness` drives each slider, swatch and
  trackball a window draws and checks the drawn sky or water follows. An e2e
  test checks a preview live with one or two knobs, not all of them.
- **The parcel environment layer is not driven yet**: the fake grid pushes a
  parcel's properties only on arrival, and the driver cannot hold a movement
  key, so walking over a parcel line changes nothing a test can see
  ([[test-e2e-environment-parcel-layer]]).
- **A key is held by frames** (`RequestBody::Press::hold_frames`, the
  driver's `hold`): down, N frames of nothing, up — what flies the flycam or
  walks the avatar. A tap is `0`.
- **The agent readout carries the camera's eye** (`camera_eye`), region-local
  to the agent's region in Second Life axes, read off the `ViewerCamera`'s
  drawn transform as the world model places an object.
- **Colours are in the model**: a colour well's value is `NodeValue::Color`
  (`#rrggbb`, alpha appended only when not opaque) and reads as its text, so
  `to_have_text("#ffffff")` waits on a swatch; a text node carries `color`,
  its `TextColor`. A fading line's alpha rides along, so compare the
  `#rrggbb` prefix. Both reach AccessKit (`color_value`, `foreground_color`).
- **A web page is a `Document`**: the browser view is named by its page's title
  (none until the page states one, `browser-view-name` meanwhile) and valued by
  its address (`SemanticValue`, the generic "a value no node shows"). A stage
  viewer starts web media only with `StageBuilder::web_media()`; in process it
  finds `sl-cef-helper` beside the binaries above cargo's `deps/`. The helper is
  a target of the viewer's own package, so every viewer build (the commit hook's
  included) has it. A test serves its own page from a loopback listener: the
  media scheme allowlist refuses `data:`.
- **A list row is named by its cells**: the model's name for a `ListItem` /
  `TreeItem` skips the controls inside it, so a row holding a Profile button
  is not called "Profile".
- **Stage options beyond the login**: `estate_manager(label)` (the Region /
  Estate window's write controls; dictates the grid), `skin(id)` (as
  `--skin`, worn and not stored) and `web_media()`. A test that reads a
  settings file the viewer writes in its own time polls it (the one file
  wait); the paths differ by backend, so find it under the viewer's
  directory.
- **A chord goes to the focus**: with a text field focused (the chat bar
  after a fill, the web address bar), Ctrl+T or Ctrl+F is the field's. Open
  a window with `open_floater` there.
- **The inventory window pages a folder only when it is opened**; a search
  asks for every unpaged folder (the reference's background fetch on a
  filter), so a test finds an item by typing its name into the window's
  search. The pages arrive over several frames and each re-flows the rows,
  so a row found the first moment can move under a click: wait for the
  inventory probe (it reads the window's model) to list the item first. Until
  something pages it, the probe sees an unopened folder as loaded and
  empty.
- **Seed an account's own items** through the region scenario's
  `setup_for_agent`, wrapping the stock hook, filed by
  `sl_fake_grid::scenario::class_folder` — the stock fixtures are somebody
  else's and read-only. A landmark onto a stage region needs the region's id
  pinned (`RegionConfig::region_id`) and its asset in `Scenario::assets`.
- **A region names its simulator** (`RegionConfig::simulator_version`);
  the About window's line follows a teleport.
- **The fake grid lands a teleport below the ground on it** (the map asks
  for height 0), as OpenSim's `ScenePresence` does; a place at or above the
  ground is kept exactly.
- **Experience pickers filter by scope**: Allowed offers land-scoped
  experiences, Blocked grid-scoped ones; the fake grid's land-scoped Arena is
  rated Moderate, so the rating filter must admit it.
- Every new crate here trips the extraction gates (`private_interfaces`,
  `must_use_candidate`, fmt, machete, cargo-about, rustdoc, `cliff.toml`,
  `CHANGELOG.md`).

## How to add …

- **a semantic role** for a custom widget: put a `Semantic`
  (`sl_viewer_ui_core::semantic`) on the widget's root at spawn — the role,
  and where the name comes from (`labelled_by` a caption node, or a Fluent
  `name_key` when no text names it). Keep open state in an `Expanded`
  marker through `sync_expanded`. A control whose caption sits beside it
  takes a `LabelledBy` (on itself or its row); an icon button a
  `ButtonSpec::name_key`. `ui_contract::every_focus_stop_has_a_contract_row`
  fails any focus stop of any registered element or floater with no role or
  no name, and any floater that is not a named window.
- **a world action**: a `WorldIntent` variant and its gesture in
  `WorldTarget::input` (`sl-viewer-automation/src/world_aim.rs`), any
  precondition as a waiting `AimStage` with its `ActionabilityCheck`, and a
  fixture test in `sl-client-bevy-viewer/src/automation_world_aim.rs` that
  the viewer does the thing to the aimed object and not to what is in front.
- **a world readout**: a field on `WorldNode` (`sl-automation-proto`),
  filled in `WorldModel` (`sl-viewer-automation/src/world_model.rs`) from
  the world layers' own bookkeeping, never from a new per-frame system. If
  it is also a criterion, add it to `WorldLocator::matches_node`. If it is
  not streamed (a name, an owner), `WorldQuery::unresolved` must also wait
  for it.
- **a state probe**: a readout type in `sl-automation-proto/src/probe.rs`, a
  reader in `sl-viewer-automation/src/probes.rs` (or, for a model in a heavy
  crate, a reader exported by that crate and registered in `ProbeSources`
  by `automation_sources.rs`), a teeth test that the probe changes when the
  state does (fixture tier in `sl-client-bevy-viewer/src/automation_probes.rs`,
  the full-stack test there for what only a real session feeds), its
  `Probe` / `ProbeReadout` variants and the executor's `state::read` arm,
  and — once the driver exists — its driver method.
- **a whole viewer App** (a transport, a harness, a stage): never assemble
  plugins by hand — `ViewerAppOptions::new(params)` is the interactive
  viewer; set `window: WindowMode::Windowless`, `storage:
  Storage::Ephemeral` (or `Storage::Directories(ViewerPaths::under(root))`
  when it must store, and one root per viewer), `audio_device: false`,
  `media: MediaRuntime::OFF`, stated `render_overrides` and — beside another
  viewer — a `log_label` for a test (`WindowMode::Headless { .. }` instead
  when it must render the UI and take clicks), then
  `ViewerAppBuilder::from_options(..).build()`
  (`sl-client-bevy-viewer/src/assembly.rs`). A new viewer-wide option
  belongs there, with its first consumer.
- **a driver verb**: a method on `UiLocator`, `WorldHandle` or `Viewer`
  (`sl-viewer-driver/src/{ui,world,viewer}.rs`) that sends one request
  through `Viewer::ask` with a `Subject` (what a failure's screenshot
  outlines) and matches the one answer it expects; an expectation in
  `expect.rs` is a wait request, never a read in a loop. A verb that needs a
  new request adds it first (see *a request*). If a person or an agent would
  use it from a shell, give it a `Verb` in `sl-viewer-ctl/src/cli.rs`, its
  arm in `verbs.rs` and an `Outcome` it prints as text and JSON, with a
  scripted-viewer test in `sl-viewer-ctl/src/tests.rs`.
- **a locator field**: its attribute in the grammar too (`selector.rs`,
  parse and print), and the field in the round-trip shapes of
  `selector/tests.rs`.
- **a request**: a `RequestBody` variant and, when it answers with
  something new, a `ResponseBody` one (`sl-automation-proto/src/message.rs`,
  with round-trip tests), any new failure as an `AutomationError` kind (and
  its arm in the executor's `ui_locator`), its arm in the executor's `start`
  and a task in `executor/{ui,world,state}.rs` — marked as acting if it
  plays input — and a test through the queue (`executor/tests.rs`, or the
  fixture world in `sl-client-bevy-viewer/src/automation_world_aim.rs`).
- **an end-to-end test**: in `sl-client-bevy-viewer/tests/`, build a
  `Stage`, launch viewers, act through locators, assert with `expect`. Take
  the lowest tier that can produce the failure (see
  [context/testing.md](testing.md)); this tier is for what only the whole
  app, or several of them, can break.
