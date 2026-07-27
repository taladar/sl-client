---
id: viewer-table-widget-remaining
title: Remaining table-widget migrations (inventory columns, full friends rights)
topic: viewer
status: ready
refs: [viewer-ui-table-widget, viewer-inventory-folder-tree, viewer-social-people-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Follow-ups deferred out of [[viewer-ui-table-widget]] (the reusable table widget
is built; group profile members/notices/roles and the People friends list are
migrated). Two migrations were parked, by agreement, because each is a larger,
higher-risk effort than the widget itself:

- **Inventory columns** — the inventory panel is a folder **tree**
  ([[viewer-inventory-folder-tree]]), not a flat sortable table. Putting its
  Name / Size / Date columns on the widget means teaching the widget (or the
  consumer) to combine the tree's indent + twisty with the table's columns —
  a genuine redesign, not a mechanical swap. Decide whether the widget grows a
  "tree column" affordance or the inventory keeps a bespoke first column.

- **Full People friends rights-grid** — today the friends list uses the widget
  for Name / Status / scroll / selection / resizable widths, and keeps the
  permission icon-grid as widget **custom** columns (its grouped
  "They can…/You can…" 2-row header and bespoke 8-way `SortState` survive
  intact). A *full* migration would move the rights into first-class widget
  columns, which needs: icon (non-text) cells, an optional **grouped / 2-row
  header**, and **per-sub-column** sort folded into the widget's own
  `TableSort`. Only worth it if a second consumer wants those features — the
  current split already delivers the ellipsis + resizable-column wins with no
  regression to a privacy-sensitive control. Needs live 2-avatar verification
  of the rights toggles either way.
