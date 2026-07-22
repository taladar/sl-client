---
id: viewer-navigation-favorites-bars
title: Navigation / location bar + favorites bar
topic: viewer
status: deferred
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-places-landmarks, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

The default-skin top chrome Vintage does without: the **navigation bar**
(back / forward / home buttons over a teleport-history stack, and the
typeable **location field** with region-name autocomplete dispatching via
[[viewer-slurl-parse-dispatch]]) and the **favorites bar** (favorite
landmarks as one-click buttons, drag-to-reorder, fed from the Favorites
folder that [[viewer-places-landmarks]] manages).

**Deferred**: the parity target is the Vintage layout, whose classic
status/top bar (already ours) carries location display instead; these bars
are default-skin chrome to revisit once Vintage parity is done. The
teleport-history *data* both need lands earlier with
[[viewer-places-landmarks]].

Reference (Firestorm, read-only): `panel_navigation_bar.xml`,
`llnavigationbar`, `panel_favorites.xml`, `llfavoritesbar`.
