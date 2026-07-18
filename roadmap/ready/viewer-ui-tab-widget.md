---
id: viewer-ui-tab-widget
title: Reusable tab widget (horizontal + vertical)
topic: viewer
status: ready
origin: noticed while building viewer-inventory-folder-tree, whose tabs are
  ad-hoc (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-inventory-folder-tree, viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

A **reusable tabbed container**: a strip of tab buttons that switches which one
of a set of panels is shown, with exactly one active. The reference viewer uses
both layouts, so the widget must support **both**:

- **Horizontal** tabs along the **top or bottom** edge (floater tabs).
- **Vertical** tabs along the **left or right** edge (the sidebar / preferences
  style).

Per the UI conventions it sizes to content (no fixed rects), reflows on a
font-size / locale change, and the left/right placement **mirrors under RTL**
([[viewer-ui-skin-tokens]] / the scaffold's `UiDirection`). Active-tab state,
`Tab`/arrow focus, and show/hide via `UiPanelShown` come from the scaffold.

**Adopt it in the existing ad-hoc tabbed panels:** the inventory window
([[viewer-inventory-folder-tree]], whose Everything / Recent / Worn tabs are
hand-rolled today) and the preferences floater ([[viewer-preferences-floater]]).

Reference (Firestorm, read-only): `lltabcontainer`.
