---
id: viewer-automation-app-builder
title: A public ViewerAppBuilder — one assembly for the binary, the harness and tests
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
refs: [viewer-plugin-groups, viewer-fake-grid-render-harness,
  viewer-automation-inprocess-transport, viewer-automation-windowless-mode]
---

## Done (2026-09-28)

`sl-client-bevy-viewer/src/assembly.rs`: `ViewerAppOptions` (pub
fields, `ViewerAppOptions::new(params)` is the interactive viewer),
`ViewerAppBuilder::from_options(..).build()` → `ViewerApp` (`run()` →
`LoginOutcome`, or `app_mut` / `into_app` for a caller that steps it). The
options that differ between the binary and a test are named:

- `WindowMode::{Windowed, Windowless}` — windowless is what the full-stack
  harness was: no window, no winit, no pipelined rendering, no device read,
  no cursor grab; only image-targeted cameras render.
- `Storage::{UserDirectories, Ephemeral}` — ephemeral: declared settings
  with no file behind them, no account dirs, no chat logs, no inventory
  cache.
- `audio_device` (the harness must not open the speakers), `media`
  (`MediaRuntime::OFF`), `render_overrides` (`None` reads the env).
- The run-scoped CLI overrides live in `content`, `capture`, `camera`,
  `skin`.

`run_viewer` / `run_replay` build through it (`cli_app_options` is the
shared CLI half). `ViewerHarness` builds through it and now runs all six
groups; its hand-assembly and its dozen stubbed resources and messages are
gone, it retargets the builder's own camera at its readback image instead
of spawning a second one. The builder also carries what no group did (the
avatar library, the fonts via the UI group, the media plugins).

Taken along: [[viewer-ui-shell-plugin-groups]] — the builder could not
give the harness "the UI and shell groups" without them existing.

Tests: the whole full-stack tier (24 tests) passes on the builder-built
app, with the background inventory crawl on as in the viewer; a new
`the_harness_runs_the_interface_too` finds the start-up floaters spawned
and closed after a fake-grid login. A `--screenshot-dir --capture-ui`
run against the local OpenSim wrote its frames with the full chrome and
logged no warning.

**Not done: "whether automation is installed".** Today that would mean
adding `SyntheticInputPlugin`, which needs a primary window a windowless
App does not have yet, and nothing drives it — an option with no consumer.
It lands with its first consumer ([[viewer-automation-windowless-mode]] or
[[viewer-automation-executor]]).

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
