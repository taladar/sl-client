---
id: viewer-skin-list-row-striping
title: Scroll-list rows — striping, hover and a selection the skin owns
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-table-widget, viewer-ui-virtualized-list, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Every scroll list in the reference viewer has four row states, and names a
colour for each one:

| State | Reference colour | Vintage |
| --- | --- | --- |
| ordinary row | `ScrollBgWriteableColor` | `#c8cfcc` |
| every other row | `ScrollBGStripeColor` | `#b8bfbb` |
| hovered row | `ScrollHoveredColor` | `#bec3c3` |
| selected row | `ScrollSelectedBGColor` | `#8d90c2` |
| disabled row | `ScrollDisabledColor` (text) | `#00000040` |

Ours has one. `ui_table.rs` `apply_table_selection_highlight` writes
`SELECTED_ROW_BACKGROUND` (a hardcoded `srgba(0.24, 0.34, 0.52, 0.55)`) for a
selected row and `Color::NONE` for every other, and `virtual_list.rs` adds
nothing. There is no striping and no hover row anywhere in the viewer.

Striping is not decoration on a list a hundred rows long — it is how the eye
keeps a row's cells together across a wide table, which is most of what an
inventory list, a radar and a region-top-objects list are.

## What to do

- Add the four row roles to the token vocabulary
  (`--list-row-bg`, `--list-row-stripe`, `--list-row-hover`,
  `--list-row-selected-bg`, `--list-row-selected-text`), alongside `--list-bg`
  from [[viewer-skin-light-surface-roles]].
- Give `virtual_list` / `ui_table` rows an even/odd distinction the cascade can
  see. The rows are recycled as the list scrolls, so parity must follow the
  **data index**, not the slot — a stripe that walks up the list as you scroll
  is worse than no stripe. `virtual_list.rs` already keeps `VirtualRow.index`
  for exactly this kind of question; note the module's own warning that
  `first + slot` is the mapping that does *not* work.
- Add the hover state on the same mechanism as the rest of
  [[viewer-skin-widget-state-classes]], rather than a fifth per-frame paint.
- Keep the `TableSelectionMode::None` escape hatch: a consumer that owns its
  row backgrounds (the transcript bands, for one) must keep them.

## Done when

A list with selection shows stripes that stay put while scrolling, a hover
row, and a selected row whose text colour the skin chose; the flat skins look
as they do now; and a test asserts stripe parity is by data index across a
scroll.
