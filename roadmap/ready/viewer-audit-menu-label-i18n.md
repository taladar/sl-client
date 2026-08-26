---
id: viewer-audit-menu-label-i18n
title: Menu and pie-menu labels cannot be translated, by type
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/viewer.md](../context/viewer.md).

`MenuCommand::label` (`sl-viewer-ui-widgets/src/menu.rs:103`) and
`PieAction::label` (`sl-viewer-ui-pie-menu/src/pie_menu.rs:456`) are
`&'static str` **text**, not keys. They are rendered by
`Text::new(cx.text(def.label))` (`menu.rs:783`, `pie_menu.rs:1323`), and
`ElementCx::text` is `SampleText::apply` (`ui_element.rs:191`) — a
pseudolocalisation / test-sample transform with **no Fluent lookup in it at
all**.

That is roughly **507 hardcoded English labels** on the viewer's two primary
interaction surfaces: 197 `label: "..."` across the four pie-menu files and 310
`MenuCommand::new(` call sites in `menu_bar.rs`, `inventory.rs`,
`inventory_actions.rs`, `radar.rs`, `blocked.rs`, `world_map.rs`, `minimap.rs`.
`sl-viewer-map` is the extreme case: 7 i18n references in 7023 lines, against
`sl-viewer-people`'s 92.

Contrast `ui_table.rs:868`, which does it right with
`Translated::new(column.header_key)`.

Scope: add `label_key` to both types, exactly as
`NotificationTemplate::message_key` already is, and resolve through
`Translator`. This is a **type** change, so it only gets more expensive.

For the record, i18n elsewhere is in good shape — only 16 hardcoded
`Text::new("...")` literals workspace-wide, and all 1310 notification message
keys resolve. Worth saying out loud somewhere, though: the three non-English
locales cover **230 of 3038** English keys (7.6%) and **none** of the
notification catalogue, so ja/ar/pl are locale-mechanism samples rather than
translations — while the pseudolocale and RTL machinery around them is genuinely
complete.

Two small companions: `i18n.rs:583` renders a missing key as the key itself
(`content.unwrap_or_else(|| key.to_owned())`) with **no `warn!`**, so a typo
ships as a literal `notification-confirm-quit` on a modal — a deduped warning
would make every uncaught typo visible in the journal. And 210 settings
descriptions across the workspace are untranslated English, surfacing in the
debug-settings editor and preferences.
