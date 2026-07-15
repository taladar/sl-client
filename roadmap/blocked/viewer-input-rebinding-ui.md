---
id: viewer-input-rebinding-ui
title: Key-binding configuration UI
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system/viewer-preferences-ui
blocked_by: [viewer-input-rebinding-persistence, viewer-input-conflict-detection, viewer-ui-text-input-widget, viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

The key-binding configuration UI, living in the preferences surface: list the
named actions **per modal context**, press-a-key-to-capture a new binding, and
reset-to-default. Because bindings are per-context
([[viewer-input-action-map]]), the UI is organised by context (mouselook /
third-person / sitting / edit / …).

**Warn before overwriting.** When the captured key is already bound to another
action in that context ([[viewer-input-conflict-detection]]), warn before
applying — name the existing binding and require confirmation — so the user
can't silently steal a binding whose row is scrolled off-screen and invisible.

Reference (Firestorm, read-only): the controls/keybindings tab in
`llfloaterpreference`, `llkeyconflict` presentation.
