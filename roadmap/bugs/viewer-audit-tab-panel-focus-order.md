---
id: viewer-audit-tab-panel-focus-order
title: Inactive tab panels keep their keyboard tab stops
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets/src/ui_tab.rs:1531` — `reconcile_tab_selection` hides a
non-selected panel with `*visibility = Visibility::Hidden` only. Nothing in
`ui_tab.rs` touches `TabIndex` (the only `TabIndex` writes are at `:663`, on the
strip).

The project's own doc states the rule this breaks
(`sl-viewer-ui-core/src/ui.rs:645`): *"`bevy_input_focus`'s tab navigation walks
the hierarchy and does **not** check visibility or display, so a closed panel's
buttons stay reachable by `Tab`"*. So every tabbed floater — preferences,
profiles, places, search, pickers, inventory — tabs focus into invisible panels.
The fix already exists next door: `ui.rs`'s `ParkedTabIndex` mechanism, which
this widget does not reuse.

Nested variant, one level up: `sl-viewer-ui-core/src/ui.rs:685` —
`apply_panel_visibility` walks
`once(panel).chain(children.iter_descendants(panel))` and, when shown, restores
**every** `ParkedTabIndex` in the subtree. A nested `UiPanelShown(false)` panel
inside it did not change, so nothing re-parks it and its widgets rejoin the tab
cycle while still `Display::None`. The one test (`ui.rs:1756`) uses a single
flat panel and cannot see this.

Both are directly testable: `sl_viewer_testkit::navigate` drives real
`NavAction::Next` and is currently used at exactly two call sites in the whole
workspace (`sl-client-bevy-viewer/src/ui_test.rs:503`, `:512`).
