---
id: viewer-menu-dropdown-rows-larger-than-reference
title: Drop-down menu rows are larger than the reference's at the same UI scale
topic: viewer
status: bugs
origin: viewer-menu-bar-twice-reference-height (2026-09-27)
refs: [viewer-menu-bar-twice-reference-height]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-menu-bar-twice-reference-height]] brought the top bar to the
reference's 19 px and 12 px text. The menus it drops are still set in the old
sizes: `ENTRY_FONT` 15 px with `ENTRY_PADDING` 10 / 5 px
(`sl-viewer-ui-widgets/src/menu.rs`). The reference's rows are 12 px text
(`widgets/menu_item.xml`, `SansSerifSmall`) and one line plus
`MENU_ITEM_PADDING` (4 px) tall, with `LEFT_PAD + LEFT_WIDTH` (18 px) before
the label and `RIGHT_PAD + RIGHT_WIDTH` (22 px) after it (`llmenugl.cpp`).

A chrome capture cannot show this: both harnesses keep menus closed. Check it
live, or with a gallery specimen, against a reference screenshot.

## Done when

An open top menu's rows have the reference's height and text size at UI scale
1, in logical pixels, so they still follow the output scale.
