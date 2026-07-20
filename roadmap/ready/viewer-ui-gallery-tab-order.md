---
id: viewer-ui-gallery-tab-order
title: Give the gallery a sensible keyboard tab order
topic: viewer
status: ready
origin: live review of viewer-ui-focus-ring-visible (2026-07-20)
refs: [viewer-ui-focus-ring-visible, viewer-ui-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

With the focus ring now visible ([[viewer-ui-focus-ring-visible]]), the
gallery's `Tab` order reads as a mess: focus jumps around the grid rather than
following the visual reading order (leading-to-trailing, top-to-bottom). The
gallery packs every registered widget specimen — plus the menu-bar, tab and pie
specimens — into a wrap grid, and almost all of them carry `TabIndex(0)`, so
`bevy_input_focus`'s tab navigation falls back to hierarchy / spawn traversal
order for the ties, which does not match where the eye expects to go next.

Wanted: a tab order in the gallery that follows the visible layout.

- First find the actual cause: is it purely the `TabIndex(0)` tie-break landing
  in spawn order that happens to differ from visual order, or does the wrap-grid
  hierarchy genuinely diverge from reading order? (Check how
  `bevy_input_focus`'s `TabNavigation` breaks ties before assuming.)
- Fix at the gallery level (this is the demo surface, not a shipped UI): assign
  ordered `TabIndex` values as cards/specimens are spawned, or spawn in reading
  order, so `Tab` walks card-by-card in the order they are laid out.
- This is gallery-scoped by default; only promote it to a scaffold concern if
  the root cause turns out to be a general widget-scaffold ordering bug that
  would bite a real floater too (e.g. the inventory).
