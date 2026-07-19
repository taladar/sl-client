---
id: viewer-ui-combo-widget
title: Reusable combo / dropdown widget
topic: viewer
status: ready
origin: split from viewer-ui-settings-binding — the third `control_name` widget
  had no composite to bind to (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-settings-binding-combo]
---

Context: [context/viewer.md](../context/viewer.md).

A reusable **combo box / dropdown**: a button showing the current selection
that, when activated, opens a popover list of named options and emits the chosen
one.
This is the reference viewer's third `control_name` control (after the checkbox
and slider that [[viewer-ui-settings-binding]] already covers), the widget a
preference like a graphics quality preset, an anti-aliasing mode or a locale
picker is built from.

Assemble it from the `bevy_ui_widgets` primitives rather than from scratch:
`Popover` for the open/close + outside-dismiss, and a `ListBox` / `Menu` of
selectable rows for the options; the closed state is a `Button` label. Follow
the scaffold conventions ([[viewer-ui-widget-scaffold]]) — logical
`start`/`end` naming, content-driven auto-layout so a long translated option
label grows the popover instead of clipping, and full `Tab` / arrow-key
reachability with the popover trapping focus while open.

The widget's value should be expressed as a **named option** (a stable key or
index the caller maps to its own type), not a bare `Entity`, so a consumer — the
[[viewer-ui-settings-binding-combo]] layer especially — can bind it to a typed
setting without threading per-row entities. Register it in the UI element
registry ([[viewer-ui-test-harness]]) so it inherits the whole check matrix.

Out of scope (its own task): the two-way binding of a combo to a setting, which
is [[viewer-ui-settings-binding-combo]].

Reference (Firestorm, read-only): `llcombobox`, `llfloater` popup handling,
`llscrolllistctrl`.
