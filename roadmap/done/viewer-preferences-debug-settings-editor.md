---
id: viewer-preferences-debug-settings-editor
title: Raw debug-settings editor
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The raw **debug-settings editor** (`llfloatersettingsdebug`): a searchable,
type-aware editor over every entry in the typed settings store — pick a setting
by name, see its type / default / current value, and edit it directly. The
escape hatch for settings that have no dedicated preferences control. Built over
the same store binding the preferences floater ([[viewer-preferences-floater]])
uses.

Reference (Firestorm, read-only): `llfloatersettingsdebug`, `llviewercontrol`.

Builds on: [[viewer-preferences-floater]] and the typed settings store.

## Done

New viewer module **`src/debug_settings.rs`** (`DebugSettingsPlugin`, floater
id `debug_settings`), a **separate floater** (user decision, reference-faithful)
opened from a new top-level **Advanced** menu ("Debug settings…") or
`Ctrl+Alt+Shift+S`:

- **Left pane**: a search box matching name *and* comment
  (case-insensitively), a "changed settings only" toggle backed by a real
  registered setting (`DebugSettingsHideDefault`, the reference's name), and
  a virtualized two-column table (`*` changed marker + name) over the store's
  sorted-name enumeration.
- **Right pane**: comment / type (+ transient marker) read-outs, the value at
  every layer (declared default, Global override, Account override,
  effective), a **scope selector** choosing which layer edits and resets
  write to (user decision — our layered store shows more than the reference's
  merged control groups), one build-once `Display`-toggled editor stack per
  `SettingKind` (checkbox; line field; Float / Integer / NonNegativeInteger
  numeric fields; X/Y/Z vector and L/T/R/B rect fields; colour swatch +
  Color4 alpha field), a copy-name button (OS clipboard) and a per-scope
  reset-to-default button.
- **Semantics**: edits apply **live** (no OK / Cancel snapshot — that is
  preferences-shell-specific); text fields commit on Enter / blur, an
  incomplete field abandons the commit; the Account layer is guarded until
  the account scope loads at login (combo disabled, edit target snapped back
  to Global). No per-edit disk write (the quick-preferences convention);
  values ride the existing logout / preferences-OK / persist-flush edges.
- **Hidden UI state**: `sl-settings` declarations grew an `editor_hidden`
  flag (`register_hidden_in`, the reference's
  `isHiddenFromSettingsEditor`) — the floater-geometry, tab-split and
  table-sort/width persistence keys register through it and the editor's
  enumeration skips them (user decision: mechanical UI state is not a
  debuggable knob).
- Plus: the `ADVANCED_MENU` in `menu_bar.rs` (after Help, the reference's bar
  order), a `debug-settings` gallery specimen (swept by the harness matrix —
  which caught two text-overflow hazards, fixed via label slots and a
  content-sized card), and Fluent keys in `en/main.ftl`.

Verified by 10 headless tests (value formatting over every kind, name+comment
filter, changed-only view, hidden-declaration skip, bool write to the selected
scope, numeric Enter/blur commit incl. abandon-on-unparsable, Vec3/Vec3d
assembly, per-scope reset, account guard snap-back, external-change label
follow) + the store's `register_hidden_in` coverage, and live on the local
grid (floater restores open via geometry/visibility persistence, list
populates sorted from the real store, detail placeholder / scope combo /
buttons laid out, Advanced menu present). Live-grid interaction checks that
need a human at the window (row click → detail, in-world effect of a live
edit, clipboard paste) remain open.
