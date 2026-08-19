---
id: viewer-inventory-search-scope-filters
title: Inventory search scope and permission filters
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-inventory-advanced-filters, viewer-inventory-search-filter]
---

Context: [context/viewer.md](../context/viewer.md).

The reference inventory gear menu offers **Search By…** modes — Name /
Creator / Description / UUID / All (`llpanelmaininventory.cpp`
`setSearchType`) — plus the permission filters **Only Modifiable / Only
Copyable / Show Transferable / Only Coalesced**, and the
search-visibility toggles from menu_inventory_search_visibility.xml:
search outfit folders, search Trash, search Library, include links.

Our filter floater (`sl-client-bevy-viewer/src/inventory_filters.rs`,
[[viewer-inventory-advanced-filters]] done) covers type / date / worn
only, and the search field ([[viewer-inventory-search-filter]] done)
matches names only. All the facts needed are already on the held items —
permissions mask, creator, description, UUID — so this is
filter-predicate plus UI work on the existing floater and search field:
a search-mode selector, the four permission checkboxes, and the four
scope toggles, each feeding the existing filter pipeline.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_inventory_gear_default.xml`,
`menu_inventory_search_visibility.xml`,
`indra/newview/llpanelmaininventory.cpp` (`onFilterEdit`,
`setSearchType`).
