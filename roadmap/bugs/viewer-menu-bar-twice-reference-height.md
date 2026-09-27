---
id: viewer-menu-bar-twice-reference-height
title: Our menu bar is twice the reference's height at the same UI scale
topic: viewer
status: bugs
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
