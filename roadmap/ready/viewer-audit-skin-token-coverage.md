---
id: viewer-audit-skin-token-coverage
title: The skin system covers two widgets
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/viewer.md](../context/viewer.md).

Across the viewer crates there are **643 hardcoded `Color::srgb*` literals**
against **98 `ClassList` attachments**, and only five files reference a `sk-`
skin class at all: `menu.rs` (8), `skin.rs` (4), `ui_search.rs` (2),
`ui_combo.rs` (1), `ui_color_picker.rs` (1). `assets/skins/common.css` defines
35 classes.

So switching skin or theme restyles the menu bar and the search box — and
nothing else. `floater.rs` (9 colours + `DOCK_HOST_BACKGROUND`), `ui_tab.rs`
(12), `ui_text_input.rs` (7), `pie_menu.rs` (8), `ui_radio.rs` (4),
`ui_table.rs` (2) and `virtual_list.rs` (2) all declare theirs in Rust, as
dark-theme values (e.g. `floater.rs:112 FLOATER_BACKGROUND = Color::srgba(0.11,
0.12, 0.15, 0.95)`). Floaters, tabs, tables, radios, text fields, combos and pie
menus are **not skinnable**.

The two worst offenders by frequency are worth promoting first:
`const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96)` is copy-pasted **20
times** workspace-wide and `DIM_LABEL_COLOR: srgb(0.62, 0.66, 0.74)` **12
times** — and three copies have already drifted (`volume_panel.rs:66`,
`quick_preferences.rs:106`, `inspector_popup.rs:118`).

This is a decision as much as a task: either widen the CSS vocabulary to cover
the widget set, or scope what "skin" means in the docs. Right now the feature's
reach and its billing do not match.

Note one legitimate exception to keep: `notification_host.rs:194 kind_accent` is
explicitly meaning-bearing ("the kind accent is painted on it in Rust", `:97`)
rather than a fallback — though the toast card's own background *is*
skin-driven, so one card currently mixes both systems.
