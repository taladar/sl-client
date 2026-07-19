---
id: viewer-preferences-floater
title: Preferences floater shell + settings store binding
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

The settings floater **shell**: the tabbed preferences window plus the binding
that wires each control to the persistent, **typed settings store**
([[viewer-ui-settings-binding]]), with sensible **defaults** and **per-account
overrides**. This is the root of the preferences cluster — the individual tabs
plug into the shell and read / write through the same store.

The per-tab content (graphics, audio, chat / privacy, camera / move-and-view,
and the raw debug-settings editor) lives in the sibling tasks that depend on
this one. Note: the input system's key-rebinding tab lives with the input
cluster, not here.

Reference (Firestorm, read-only): `llfloaterpreference*`, `llviewercontrol`
(settings backend), `fspanelprefs`.

Builds on: [[viewer-ui-settings-binding]].
