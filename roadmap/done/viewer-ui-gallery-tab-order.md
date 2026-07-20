---
id: viewer-ui-gallery-tab-order
title: Give the gallery a sensible keyboard tab order
topic: viewer
status: done
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

## Done

Cause (found via a debug dump of the focus path and the box of each stop): there
are **no** nested `TabGroup`s, so the whole gallery is one hierarchy walk that
`bevy_input_focus` stable-sorts by `TabIndex` — and with almost every stop at
`TabIndex(0)` the order is pure hierarchy/spawn order. That order diverges from
the screen because the gallery packs stops from sources whose spawn position
does not track their layout position: the `floater` specimen parents to the
**root** (not its card), the header switcher sits above the page, and so on. The
main element cards were actually already in order; a handful of reparented stops
jumped the sequence.

Fix (gallery-only, `order_gallery_tab_stops`): a `PostUpdate`
`.after(UiSystems::Layout)` system re-numbers every focus stop by its live
on-screen position — `UiGlobalTransform.translation` sorted top-to-bottom then
leading-to-trailing (`f32::total_cmp`, a total order so a settled layout never
oscillates) — assigning each a rank as its `TabIndex`. It re-derives every frame
(a `!=` guard makes it a no-op once settled), so a cell-change respawn or a
font-size reflow re-sorts for free. Chose position-sort over hand-assigned
indices because it is cause-agnostic: it fixes reparented / out-of-hierarchy
stops that per-spawn indices would miss.

Deliberately **not** promoted to a scaffold concern: a real UI (the viewer's
floaters, the inventory) orders its own stops deliberately at spawn; only the
gallery, which aggregates every specimen from every source, needs the positional
re-sort. Live-confirmed: `Tab` now walks the gallery top-to-bottom.
