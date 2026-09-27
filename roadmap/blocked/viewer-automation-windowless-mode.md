---
id: viewer-automation-windowless-mode
title: Windowless viewer — the whole app, UI included, with no OS window
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 13
blocked_by: [viewer-automation-app-builder, viewer-automation-offscreen-window-spike]
refs: [viewer-automation-synthetic-input]
---

Context: [context/automation.md](../context/automation.md).

A test viewer must not need a compositor, steal focus, or read the user's
real mouse and keyboard (a screenshot run already did). *How* it renders
and picks without a window is decided by
[[viewer-automation-offscreen-window-spike]]; this task builds it into the
viewer.

## Wanted

- `--headless` (a runtime switch and a `ViewerAppBuilder` option, never a
  Cargo feature): no winit, the primary window replaced per the spike's
  decision, every camera (world, HUD, UI, gizmos) rendering at the capture
  size, a `ScheduleRunnerPlugin` at a fixed rate that never throttles.
- **One off-screen path**: unify with the screenshot harness
  (`sl-viewer-world-view/src/screenshot.rs`, `SL_VIEWER_SCREENSHOT_*`,
  `--capture-*`) rather than growing a second one.
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
  (the screenshot harness's preview quad), for a human to follow.
- AccessKit needs a winit window, so it is absent headless; nothing in the
  semantic model may depend on it.

Acceptance: the viewer logs into the fake grid with `--headless` on a
machine with no Wayland or X display, renders UI and world, and a synthetic
click opens a floater and a synthetic right-click on a prim opens its pie;
`--watch` shows the same run; moving the real mouse during a watched run
changes nothing.
