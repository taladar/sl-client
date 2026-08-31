---
id: viewer-ui-widget-interaction-suite
title: Deep interaction tests for the stateful widgets
topic: viewer
status: ready
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 8
blocked_by: [viewer-ui-interaction-harness, viewer-ui-keyboard-text-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The contract sweep proves reactions exist; the stateful widgets need
scenario tests on top:

- `virtual_list.rs` scrolling via real `AccumulatedMouseScroll` +
  `HoverMap` — the row virtualisation window moves, no dead rows;
- `ui_combo` open/select/dismiss;
- `ui_color_picker` and `ui_texture_picker` drag-on-gradient;
- `ui_table` header-click sorting;
- `ui_tab` switching (all four edge orientations);
- `ui_search` type-to-filter;
- `menu.rs`/`menu_bar.rs` open-navigate-dismiss, including keyboard
  navigation and accelerators;
- divider/resize drags where panels have them.

Each widget test drives the same synthetic pointer/keyboard and asserts
emitted actions plus widget state; committed pinning tables where the
widget has an address space (menu items, combo entries).
