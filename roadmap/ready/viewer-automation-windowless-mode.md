---
id: viewer-automation-windowless-mode
title: Windowless viewer — the whole app, UI included, with no OS window
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 13
blocked_by: [viewer-automation-app-builder, viewer-automation-offscreen-window-spike]
refs: [viewer-automation-synthetic-input]
---

Context: [context/automation.md](../context/automation.md).

A test viewer must not need a compositor, steal focus, or read the user's
real mouse and keyboard (a screenshot run already did).
[[viewer-automation-offscreen-window-spike]] decided *how* it renders and
picks without a window: the primary window is a `Window` carrying the Bevy
fork's `OffscreenWindow` marker — no platform window, an off-screen texture as
its swap chain, every camera keeping its window target. Its prototype is the
full-stack harness's `HarnessOptions::in_offscreen_window` (spawn the window
with `PrimaryWindow` + `OffscreenWindow` and a scale factor of 1, install
`SyntheticInputPlugin`, screenshot the window each frame); this task builds it
into the viewer.

## Wanted

- `--headless` (a runtime switch and a `ViewerAppBuilder` option, never a
  Cargo feature): no winit, the primary window an `OffscreenWindow` at the
  capture size with its scale factor pinned to 1 (so every camera — world,
  HUD, UI, gizmos — renders at the capture size through its unchanged window
  target), `SyntheticInputPlugin` installed, a `ScheduleRunnerPlugin` at a
  fixed rate that never throttles. Move the prototype out of the harness into
  the builder (`WindowMode::Offscreen`, or `Windowless` gaining the window),
  and have the full-stack harness build through it.
- **One off-screen path**: unify with the screenshot harness
  (`sl-viewer-world-view/src/screenshot.rs`, `SL_VIEWER_SCREENSHOT_*`,
  `--capture-*`) rather than growing a second one — under `--headless` a
  capture is `Screenshot::primary_window()` of the off-screen window, and the
  capture cameras' per-run image retargeting is only needed for a *real*
  window whose size the compositor decides.
- **Input isolation**, headless *and* under `--watch`: drop window input
  events, and build without the device plugins
  (`ViewerInputPlugins::without_devices()` — evdev, SpaceNavigator) and
  gamepads, so nothing but the synthetic injector moves the viewer.
- Window-bound services become per-App fakes or no-ops: cursor grab
  (`input_context.rs`, `pie_menu.rs`), title and resize, IME area
  (`media_ime.rs`), and the clipboard (`sl-viewer-platform/src/clipboard.rs`,
  `arboard`) — replaced by a per-App in-memory clipboard, so copy/paste is
  testable and never touches the user's.
- `--watch`: the same run with a real window showing the capture target
  (the screenshot harness's preview quad), for a human to follow. The
  off-screen window's texture lives in the render world (it is not an `Image`
  asset); it is created `TEXTURE_BINDING`, so a render-world blit onto the
  watch window can sample it, or the preview can show the screenshot. The
  watch window is a second, winit-backed `Window` that is **not** primary
  (so no cursor reader follows the real mouse over it), and `bevy_winit`
  already skips the `OffscreenWindow` one.
- AccessKit needs a winit window, so it is absent headless; nothing in the
  semantic model may depend on it.

Acceptance: the viewer logs into the fake grid with `--headless` on a
machine with no Wayland or X display, renders UI and world, and a synthetic
click opens a floater and a synthetic right-click on a prim opens its pie;
`--watch` shows the same run; moving the real mouse during a watched run
changes nothing.
