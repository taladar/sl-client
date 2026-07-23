---
id: viewer-f3-overlay-covered-by-menu-bar
title: F3 mesh/texture render display partially covered by the menu bar
topic: viewer
status: bugs
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
