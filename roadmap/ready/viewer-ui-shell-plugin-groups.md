---
id: viewer-ui-shell-plugin-groups
title: The two plugin groups the group carve-up left behind — UI and shell
topic: viewer
status: ready
origin: the viewer-plugin-groups work (2026-08-30) named six groups and landed four
points: 5
refs: [viewer-plugin-groups, viewer-ui-baseline-regressions, viewer-test-baseline-format]
---

Context: [context/testing.md](../context/testing.md).

[[viewer-plugin-groups]] planned six `PluginGroup`s and landed four —
`ViewerInputPlugins`, `ViewerRenderPlugins`, `ViewerWorldPlugins`,
`ViewerEditPlugins`. The other two are still an inline list in
`lib.rs::run_session`:

- `ViewerUiPlugins` — scaffold, i18n, widgets, panels, floaters, toolbar,
  notifications.
- `ViewerShellPlugins` — the client config, audio, CEF/media, web auth,
  clipboard, persistence, snapshot, screenshot, diagnostics.

That is not only tidiness. **Nothing can stand the viewer's UI up
headlessly**, because every floater is spawned by its own plugin from an
inline `FloaterSpec` and there is no list of those plugins to add. The
concrete casualty so far: [[viewer-test-baseline-format]] could not record
floater default sizes — a fact that is load-bearing for muscle memory,
cheap to move by accident, and named in [[viewer-ui-baseline-regressions]]
— because measuring it means asking a live app what its floaters opened
at, and the alternative (a hand-maintained table of every crate's spec)
duplicates exactly what the baseline exists to stop duplicating.

The same shape blocks any UI-level full-stack test: a login through
`sl-fake-grid` that asserts what the *interface* did on arrival, rather
than what the world did.

Both groups move **verbatim, with their comments**, as the first four did;
a plugin never appears in two lists, and a split plugin's ECS half is what
a headless app takes.

Acceptance: the `lib.rs` diff is a pure move; the settings golden and a
`--screenshot-dir` smoke run against the local grid are unchanged; a
headless app built from `ViewerUiPlugins` alone spawns the floaters and
can be asked their `Floater` state.
