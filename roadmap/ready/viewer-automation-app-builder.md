---
id: viewer-automation-app-builder
title: A public ViewerAppBuilder — one assembly for the binary, the harness and tests
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 8
refs: [viewer-plugin-groups, viewer-fake-grid-render-harness,
  viewer-automation-inprocess-transport, viewer-automation-windowless-mode]
---

Context: [context/automation.md](../context/automation.md).

The real viewer App is assembled in the private `run_session` in
`sl-client-bevy-viewer/src/lib.rs`: `DefaultPlugins`, `SlClientPlugin`, the
UI, skin and i18n plugins, about a hundred feature plugins and the plugin
groups of `viewer_plugins.rs`. `ViewerHarness`
(`full_stack_test.rs::build_viewer_app`) re-assembles a subset by hand and
stubs the resources and messages of the UI, shell and edit groups it leaves
out. So the full-stack tier never runs the app the user runs, and an
in-process automation transport would have to copy the assembly a third time.

## Wanted

- Extract the assembly into a public `ViewerAppBuilder` with explicit
  options: login params (or offline / replay), client directories, windowed
  vs windowless, capture size, whether automation is installed, and the
  run-scoped overrides the CLI sets today (camera pose, FOV, day position).
- `run_session` becomes `ViewerAppBuilder::from_options(..).build().run()`;
  `ViewerHarness` builds through it and gains the UI, shell and edit groups
  it currently stubs (keep the harness's own additions: readback, the pinned
  day, recorded events).
- Things `run_session` brings that no plugin group does (avatar asset
  library, bundled fonts, media plugins — listed in
  [context/testing.md](../context/testing.md)) move into the builder so no
  caller can forget them.

Acceptance: the binary, `ViewerHarness` and a test can each build the full
viewer App through one function; the full-stack tests pass with the UI
groups installed; no behaviour change in the interactive viewer.
