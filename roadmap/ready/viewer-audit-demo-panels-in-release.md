---
id: viewer-audit-demo-panels-in-release
title: Five developer demo panels ship in the release binary from inside library crates
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 3
refs: [viewer-audit-binary-module-extraction, viewer-audit-render-fixtures-crate]
---

Context: [context/viewer.md](../context/viewer.md).

Five `F4`-`F8` demo panels are registered unconditionally in their plugins'
`build`, with no feature gate and no `#[cfg(test)]`:

- `sl-viewer-ui-core/src/ui_text.rs:56` (F4);
- `sl-viewer-ui-core/src/ui.rs:930-1344` (F5, ~415 lines);
- `sl-viewer-ui-core/src/i18n.rs:726-1145` (F6, ~420);
- `sl-viewer-ui-widgets/src/settings_binding.rs:641-1017` (F7, ~377);
- `sl-viewer-ui-widgets/src/ui_text_input.rs:1008-1250` (F8, ~243).

That is ~1500 lines of by-hand proof surface — with hardcoded English titles
such as `Text::new("Reset to defaults")` (`settings_binding.rs:902`) — inside
the two crates that 22 other crates depend on.

Also a stale comment: `ui_text_input.rs:1009` says the panel is `F6`, but
`:1018` binds `KeyCode::F8` (F6 is the i18n demo).

Scope: feature-gate them, or move them into the gallery binary alongside the
other harness surfaces — see [[viewer-audit-binary-module-extraction]], which
covers the same problem for `gallery.rs` / `render_gallery.rs` /
`ui_elements.rs` in the viewer crate, and [[viewer-audit-render-fixtures-crate]]
for `render_scene.rs`.
