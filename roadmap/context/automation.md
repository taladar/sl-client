# Context — viewer automation (the end-to-end driver)

Non-task prose for the `viewer-automation-*` and `test-e2e-*` tasks: a
mechanism, modelled on browser test frameworks (Playwright, Selenium), that
drives the **real viewer** and reads its state back with no human in the
loop. Read [context/testing.md](testing.md) first — this adds a tier on top
of the ones described there, it does not replace them.

## Why

Every tier below this one cuts the app somewhere. `InteractionTest` and
`WorldTest` drive *fragments* of the viewer and address widgets by `Name`
string; `ViewerHarness` runs the real grid path but stubs the UI, shell and
edit plugin groups; `sl-crosscheck` runs the real binary but can only pass it
flags and read files it wrote. Nothing can open a floater in the real binary,
click a disabled button and see that nothing happened, or have two logged-in
viewers watch each other. So the last step of verifying a feature is still a
person logging in and clicking. This tier is what replaces that person.

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
| `sl-viewer-automation` | in-viewer plugin: model, input, executor | [[viewer-automation-executor]] and its blockers |
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
  wrappers, one step per frame, exactly as `interact.rs` established. A
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
- **Rendering and picking without a window is unsolved.** `bevy_ui` and
  `bevy_picking` only hit cameras whose render target equals the pointer's,
  so UI cameras rendering into an `Image` get no hits from a window pointer;
  about 25 viewer files read `Window::cursor_position()`; a `Window` with no
  raw handle renders nothing. That is why `InteractionTest` picks but never
  renders and the screenshot harness renders but never clicks.
  [[viewer-automation-offscreen-window-spike]] decides the design
  (recommended: a surfaceless window rendered off-screen in the Bevy fork).
- Headless input isolation must also drop the device plugins (evdev,
  SpaceNavigator) and gamepads, not only window events; the OS clipboard is
  replaced by a per-App one so copy/paste is testable and never touches the
  user's.
- AccessKit needs a winit window, so it is absent headless; the semantic
  model must never depend on it.
- Several statics are process-wide (`STARTUP_OVERRIDES`,
  `REPLAY_CACHE_ROOT`, `MEDIA_ENGINE_PROFILE`, `TERMINATION_REQUESTED`, env
  `OnceLock` switches), as are Bevy's tracing subscriber and task pools; two
  viewers in one process need the former per App
  ([[viewer-automation-per-app-state]]).
- Every new crate here trips the extraction gates (`private_interfaces`,
  `must_use_candidate`, fmt, machete, cargo-about, rustdoc, `cliff.toml`,
  `CHANGELOG.md`).

## How to add …

- **a semantic role** for a custom widget: put a `Semantic` component on the
  widget's root at spawn (role, and a name key when no child text names
  it); the registry sweep over `ELEMENTS` and `FLOATERS` fails any
  interactive entity with no role or name.
- **a state probe**: a request variant in `sl-automation-proto`, a reader in
  `sl-viewer-automation`, a driver method, and a teeth test that the probe
  changes when the state does.
- **an end-to-end test**: in `sl-client-bevy-viewer/tests/`, build a
  `Stage`, launch viewers, act through locators, assert with `expect`. Take
  the lowest tier that can produce the failure (see
  [context/testing.md](testing.md)); this tier is for what only the whole
  app, or several of them, can break.
