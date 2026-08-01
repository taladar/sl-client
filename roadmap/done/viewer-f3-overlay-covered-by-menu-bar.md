---
id: viewer-f3-overlay-covered-by-menu-bar
title: F3 mesh/texture render display partially covered by the menu bar
topic: viewer
status: done
origin: user report (2026-07-23)
---

Context: [context/viewer.md](../context/viewer.md).

The **F3 mesh/texture render display** (the streaming/decode diagnostic
overlay) is now **partially covered by the top menu bar**: the bar spans
the full window width (the one-full-width-bar arrangement) and renders at
a high z-index (`TOP_BAR_Z` 9000), and the overlay's top edge sits under
it.

Fix direction: lay the overlay out below the top bar (offset its top by
the bar's height — ideally by reading the bar's measured layout height
rather than a constant, so a font-size change keeps them apart) instead
of raising its z-index over the bar (diagnostics should not cover the
menus either). Check the other debug overlays for the same collision
while there.

## Resolution (done)

The pipeline (F3) overlay (`diagnostics.rs`) now offsets its root node's `top`
by `DIAG_TOP_INSET` (`42.0`), starting it **below** the full-width top bar
rather than raising its z-index over it — the intended strategy. The former
top-right FPS / frame-budget overlay was moved into the status bar (part of the
top bar), so it no longer collides either. Caveat kept as a minor follow-up:
`DIAG_TOP_INSET` is a hardcoded constant, not the bar's *measured* height, so a
large UI-font / locale reflow that grows the bar could clip the panel's first
line again — acceptable for now (the bar height is effectively fixed).
