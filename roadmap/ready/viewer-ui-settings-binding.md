---
id: viewer-ui-settings-binding
title: Two-way widget↔settings binding layer
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

The two-way binding layer that connects a widget to a named setting in the
[[viewer-ui-settings-store]] — the `control_name=` idiom the reference uses
1,293 times, which is *why* ~20 of its preference panels have almost no code
behind them: a checkbox/slider/combo declares which setting it edits, and the
binding keeps widget and store in sync (read on build, write on change, react to
external changes).

This is what makes [[viewer-preferences-floater]], [[viewer-quick-preferences]]
and the [[viewer-input-rebinding-ui]] mostly declarative rather than hand-wired.
It depends on both the widget scaffold (the widgets) and the settings store (the
values).

Reference (Firestorm, read-only): `llui` `control_name` handling, `lluictrl`
`setControlName`, `llviewercontrol` connections.
