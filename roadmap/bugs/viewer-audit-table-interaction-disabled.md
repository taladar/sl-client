---
id: viewer-audit-table-interaction-disabled
title: The table widget ignores InteractionDisabled
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`grep InteractionDisabled sl-viewer-ui-widgets/src/ui_table.rs` returns zero
hits, yet `sort_header_on_press` (`:1121`), `resize_border_on_drag` (`:1147`)
and `select_table_row_on_press` (`:1455`) all act on a press with no disabled
check.

`ui_combo.rs:288`, `ui_radio.rs:328`, `ui_text_input.rs:872` and
`ui_color_picker.rs:116` all **do** honour it, so this is an inconsistency
rather than a policy. `ui_tab.rs`, `ui_search.rs` and `floater.rs` are likewise
zero-hit and should be checked in the same pass.

The project's rule is that `InteractionDisabled` is advisory in Bevy and each
widget must honour it in its own press/change observers.
