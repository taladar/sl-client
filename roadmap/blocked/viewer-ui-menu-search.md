---
id: viewer-ui-menu-search
title: Menu search field
topic: viewer
status: blocked
origin: split from viewer-ui-menu-bar's status area (2026-07-20)
blocked_by: [viewer-ui-menu-bar, viewer-ui-text-input-widget]
refs: [viewer-ui-status-bar]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's **menu search** — a small text field in the status area
(`search_menu_edit` in `panel_status_bar.xml`, driven by
`LLStatusBar::onUpdateFilterTerm`) that finds an action *within the menus*: type
a few letters and the menu bar filters to the entries whose label matches, so a
command is reachable without knowing which menu holds it.

Builds directly on the menu widget ([[viewer-ui-menu-bar]] / the `crate::menu`
mechanism): the menus are already a **tree of data** (`MenuDef` / `MenuCommand`
with a stable `action` string and a `label`), so a search is a pure filter over
that tree — walk the whole bar's entries, match the query against each label
(and, usefully, against the `action` id and the `>`-joined menu path), and
present the hits. Two presentations worth considering, in reference-fidelity
order: (1) filter the open menu / dropdowns to the matching entries the way the
reference does, and (2) a flat results list of `menu-path > label` that
activates the entry's `action` directly (arguably better than the reference,
since the tree walk already produces the path). Activating a hit writes the same
`UiAction { element, action }` the menu would, so no per-entry rewiring.

Needs a single-line text input, so it waits on [[viewer-ui-text-input-widget]].
Belongs beside the other status-area elements ([[viewer-ui-status-bar]]) but is
its own task because it is a *menu* interaction (a filter over the menu tree),
not a status read-out.

Reference (Firestorm, read-only): `indra/newview/llstatusbar.{h,cpp}`
(`mFilterEdit`, `onUpdateFilterTerm`, `updateMenuSearchPosition`),
`newview/skins/default/xui/en/panel_status_bar.xml`.
