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
- Several statics are process-wide (`STARTUP_OVERRIDES`,
  `REPLAY_CACHE_ROOT`, `MEDIA_ENGINE_PROFILE`, `TERMINATION_REQUESTED`, env
  `OnceLock` switches), as are Bevy's tracing subscriber and task pools; two
  viewers in one process need the former per App
  ([[viewer-automation-per-app-state]]).
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
  stays where that left it.
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
  Storage::Ephemeral`, `audio_device: false`, `media: MediaRuntime::OFF`
  and stated `render_overrides` for a test (`WindowMode::Headless { .. }`
  instead when it must render the UI and take clicks), then
  `ViewerAppBuilder::from_options(..).build()`
  (`sl-client-bevy-viewer/src/assembly.rs`). A new viewer-wide option
  belongs there, with its first consumer.
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
