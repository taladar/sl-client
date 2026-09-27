---
id: viewer-menu-bar-twice-reference-height
title: Our menu bar is twice the reference's height at the same UI scale
topic: viewer
status: done
origin: test-crosscheck-pin-ui-scale (2026-09-27)
refs: [test-crosscheck-pin-ui-scale, viewer-vintage-ui-chrome-crosscheck,
       viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

In the recorded Vintage chrome pair
(`crosscheck-runs/catalogue-chrome-vintage-vs-vintage-4k`, 3840×2160, Vintage
on both sides), our menu bar is **38 px** tall and the reference's is **19 px**
(column x = 1300 of `frame_000.png`: our bar colour runs y 0–37, theirs y 0–18,
with the reference's navigation bar below it). The menu labels are larger too:
"Avatar" is about 46 px wide here and 36 px there.

This was first put down to UI scale, with ours supposedly following the 4K
output's 1.5 while Firestorm under Xwayland saw 1.0. That premise is wrong.
Both viewers draw a UI capture at an effective scale of **1.0**:

- **Ours:** a capture routes the UI camera into the off-screen image, and an
  `ImageRenderTarget` made from a handle has `scale_factor` 1.0. bevy_ui
  multiplies that by `UiScale`, which is 1.0 in a fresh run directory. The
  window's output scale plays no part.
- **Reference:** `UIScaleFactor` (1.0) × `getSystemUISize()` (1.0 under
  Xwayland).

So the difference is our own layout. Skins do not change layout here, so it is
not Vintage's doing: every skin's bar has this height.

## The work

- Find where the top bar's height comes from: its padding, the menu label font
  size, and any fixed height in the top-bar layout.
  Compare them with the reference's menu bar
  (`MENU_BAR_HEIGHT` in `llui/llmenugl`, and the menu bar's font and item
  padding in the skin's XUI).
- Bring the height and the label size to the reference's at scale 1.0, then
  re-capture the chrome pair with `--ui-scale 1` (the default for a UI run
  since [[test-crosscheck-pin-ui-scale]]).

## Done when

A Vintage chrome pair has both menu bars the same height in pixels, with both
scene dumps reporting `ui_scale` 1.

## Where the 38 px came from (2026-09-27)

Not the menu names alone. The bar is a centred flex row, so its tallest child
sets its height, and that was the **menu search box**: a 15 px line inside the
field's 6 px padding, the box's 3 px padding and a 1 px border on each side,
about 37 px. The names themselves were 15 px text with 6 px of block padding
(about 30 px), and the status read-outs 14 px text.

The reference's numbers, all at scale 1.0:

| Part | Reference | Source |
| --- | --- | --- |
| bar height | 19 px | `main_view.xml` `menu_bar_holder` |
| text | 12 px (`SansSerifSmall`, 9 pt at 96 dpi) | `widgets/menu_item.xml`, `fonts.xml`, `FontScreenDPI` |
| item height | one line (15 px) + 4 px | `llmenugl.cpp` `MENU_ITEM_PADDING` |
| item width | label + 25 px | `LEFT_PAD + LEFT_WIDTH + RIGHT_PAD` |
| search box | 18 px | `panel_status_bar.xml` `search_menu_edit` |
| status text | 12 px | `panel_status_bar.xml` |

The fix sets all three children's text at 12 px (`menu_bar::TOP_BAR_FONT`),
gives bar buttons the reference's padding (12.5 / 2), adds a compact search
box and text field (no block padding, floored at 18 px so the clear button
cannot grow it) and floors the bar at 19 px. A unit test lays the real bar out
at window scale factors 1 and 1.5.

### The output scale is ours to honour

The sizes are **logical** pixels. On a Wayland output scaled by 1.5 (the 4K
screen here) the live window draws the bar 28.5 physical px tall, while the
reference, an X11-only client under Xwayland, stays at 19. That difference is
deliberate: following the output's scale is correct behaviour the reference
simply cannot have. Only a capture, which draws into an off-screen image at
scale 1 (with `--ui-scale` pinning `UiScale`), compares the two pixel for
pixel, and that is where "the same height" is judged.

## Verified (2026-09-27)

Chrome pair `crosscheck-runs/catalogue-chrome-vintage-4k-menubar-fix`
(3840×2160, Vintage on both sides, `--ui-scale 1`): column x = 1300 of
`frame_000.png` has the bar colour on rows 0–18 in **both** frames, 19 px
each, and both scene dumps report `ui_scale` 1. The menu names now match the
reference's size and spacing by eye.

Left as it is, not part of this bug: the drop-down menus' rows are still 15 px
text (the reference's are 12 px too), and our menu search has no magnifier
glyph.
