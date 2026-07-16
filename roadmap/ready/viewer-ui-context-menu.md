---
id: viewer-ui-context-menu
title: Line-based context menu widget
topic: viewer
status: ready
origin: noticed as a missing fundamental while reviewing viewer-ui-widget-scaffold (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-radial-menu, viewer-object-context-menu, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The **general mechanism** for a conventional line-based context / popup menu —
the other half of [[viewer-ui-radial-menu]], and the shape every non-world menu
wants (a menu bar, an inventory item's right-click, a list row's actions).

Much smaller than the pie, because upstream has most of it: `bevy_ui_widgets`
ships headless `MenuPopup`, `MenuItem`, `MenuButton`, `MenuLayout` and a
`PopoverPlugin`, all keyboard-navigable already. The work is largely to style
them against [[viewer-ui-skin-tokens]] and add what a viewer menu needs on top:
separators, checked / radio items, disabled items with a reason, keyboard
accelerators shown against their entry, and submenus.

**Both menu shapes must reach the same entries.** The reference makes pie vs.
line a *preference* (`UsePieMenu` in `newview/app_settings/settings.xml`), not
two separate feature sets, and so should we: a menu is a tree of entries, and
the two widgets are two presentations of it. Design the entry model here (or in
whichever of the two lands first) so it is shared rather than duplicated —
otherwise the object pie and the object context menu drift apart, which is
exactly what happened upstream.

Per the scaffold's conventions: no physical `left` / `right` in the API or the
styling — a submenu opens toward the **inline end** and flips at the screen
edge, which under an RTL locale is the other side of the screen with no separate
code.

Deliberately **not** in scope: which entries any given menu holds
([[viewer-object-context-menu]] and the other per-domain tasks).

Reference (Firestorm, read-only): `indra/llui/llmenugl.{h,cpp}` (`LLMenuGL`,
`LLMenuItemGL`, `LLContextMenu`), `newview/llviewermenu.cpp`, and the
`menu_*.xml` layouts under `newview/skins/` as a feature checklist.
