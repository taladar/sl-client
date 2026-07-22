---
id: viewer-toolbar-customization
title: Toolbar customization (toybox)
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-vintage-bottom-bar]
---

Context: [context/viewer.md](../context/viewer.md).

The "toybox": customise which command buttons the toolbar shows and in what
order — drag buttons from a palette floater onto the bar, drag to reorder,
drag off to remove, with icon/label display modes and per-account
persistence. Our bottom toolbar exists with a fixed wired/placeholder set;
this task turns its content into a **command registry** (id, icon, label,
toggle-vs-action, target floater/action) plus the palette floater and the
drag interactions.

Note the Vintage decision: the fixed classic bottom-bar arrangement
([[viewer-vintage-bottom-bar]]) stays our default; customization must
coexist with it (the flexible toolbar remains available without breaking the
classic layout, as Firestorm's Vintage skin itself does).

Reference (Firestorm, read-only): `llfloatertoybox`, `lltoolbarview`,
`floater_toybox.xml`, `panel_toolbar_view.xml`, `toolbars.xml` (default vs
vintage command sets).

Builds on: the bottom toolbar (`bottom_toolbar.rs`) and the settings store.
