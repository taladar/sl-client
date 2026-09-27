---
id: viewer-menu-dropdown-rows-larger-than-reference
title: Drop-down menu rows are larger than the reference's at the same UI scale
topic: viewer
status: done
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

## The reference's drop-down (2026-09-27)

All at scale 1, from `llmenugl.cpp` and the default skin's widget XUI:

| Part | Reference | Source |
| --- | --- | --- |
| text | 12 px (`SansSerifSmall`) | `widgets/menu.xml`, `widgets/menu_item.xml` |
| row height | one line (15 px) + 4 px | `LLMenuItemGL::getNominalHeight`, `MENU_ITEM_PADDING` |
| before the label | 3 px pad + 15 px check column | `LEFT_PAD_PIXELS`, `LEFT_WIDTH_PIXELS` |
| after the label | 15 px slot + 7 px pad | `RIGHT_WIDTH_PIXELS`, `RIGHT_PAD_PIXELS` |
| label to accelerator | 15 px | `shortcut_pad` in `widgets/menu.xml` |
| submenu arrow | right-aligned 7 px from the edge | `LLMenuItemGL::draw` |
| separator | 8 px, rule at mid-height, 6 px short of each side | `SEPARATOR_HEIGHT_PIXELS`, `LLMenuItemSeparatorGL::draw` |
| menu | rows from the top edge, 4 px below them, no side padding | `LLMenuGL::arrange` |
| menu width | the widest row; no minimum | `LLMenuGL::arrange` (`preferred_width` only caps) |

Ours were 15 px text, 10 / 5 px row padding plus a 4 px column gap, a 16 px
check gutter and 6 px gap before the label, 24 px before the accelerator, 4 px
popup padding on every side, 9 px separators and a 140 px minimum width.

## Fixed (2026-09-27)

`sl-viewer-ui-widgets/src/menu.rs` now builds every drop-down row to those
numbers: 12 px text, 2 px block padding floored at 19 px (a floor, as the top
bar's is, so a larger UI font still grows it), a 3 px leading pad and 15 px
check gutter, and a 15 px trailing slot plus 7 px pad on every row — the
submenu arrow sits at the slot's trailing edge, and an accelerator, 15 px after
its label, ends 22 px short of the row's edge. The inline pads are logical, so
they swap sides under RTL. Separators are an 8 px band with the rule 3 px down
and 6 px in from each side; the popup pads only 4 px below its rows and has no
minimum width.

A layout test lays the fixture Avatar menu out at window scale factors 1 and
1.5 and checks, in logical px, every row's height, text size and label inset,
every accelerator's trailing inset, the separators' height and the menu's
height.

### Aiming at a submenu (2026-09-27)

Rows at 19 px instead of about 30 made a weakness of ours easier to hit: a
submenu was open exactly while its branch row was under the pointer, so a hand
moving diagonally toward it closed it on the first sibling row it crossed. The
reference avoids that in `LLMenuGL::handleHover`: it smooths the pointer's
velocity and keeps the menu's selection while that velocity points at the
submenu's side with a slope of at most `MAX_MOUSE_SLOPE_SUB_MENU` (0.9).

`MenuAim` in `menu.rs` ports it. `manage_submenus` keeps an open submenu the
pointer is heading for, opens no sibling branch meanwhile, and
`highlight_menu_hover` keeps the held branch lit instead of the row being
crossed. Two differences from the reference, both on purpose: the side is read
from where the submenu actually opened, not assumed to be the right, so it
holds under RTL and when a submenu flips to fit the window; and the velocity
is kept in fractional logical px rather than rounded to whole pixels each step.

Unit tests cover the heading maths (a shallow sweep aims at its own side only,
a pause keeps the heading, a sharp turn replaces it); a pointer scenario opens
the fixture's submenu, cuts diagonally across the sibling below toward it and
checks it stays open with the branch lit, then heads away and checks it
closes.

The 140 px minimum menu width stays gone: it did nothing for hovering, and the
reference sizes a menu to its widest row, CJK menus included.

## Verified (2026-09-27)

Live on the local OpenSim: the top menus' rows read at the new size, and a
diagonal toward an open submenu across the rows below its branch keeps it open.
