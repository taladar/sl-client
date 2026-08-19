---
id: viewer-inventory-favorites
title: Inventory favorites — star items, folders and outfits (AISv3)
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-inventory-row-decorations, viewer-inventory-folder-tree,
  viewer-outfit-editor, viewer-wearable-favorites,
  viewer-navigation-favorites-bars]
---

Context: [context/viewer.md](../context/viewer.md).

The LL-viewer inventory-favorites feature Firestorm adopted: **Add to
Favorites / Remove from Favorites** on any inventory item or folder
(menu_inventory.xml, `Inventory.DoToSelected add_to_favorites`), a star
decoration on favorited rows, outfit favorites ("Add to favorite
outfits" in menu_outfit_gear.xml, "Sort favorites to top" in
menu_outfit_gallery_sort.xml / menu_outfit_list_sort.xml), and the
Wearing-tab favorite entries. The FS Inventory Settings floater
(`floater_inventory_settings.xml`) configures how favorites display:
solid star, hollow star, and/or tinted text
(InventoryFavoritesUseStar / UseHollowStar / ColorText).

The flag persists as the AISv3 `favorite` item field, so it roams
between viewers on the same account. We have none of it — no favorite
handling anywhere in our inventory modules
(`sl-client-bevy-viewer/src/inventory.rs`, `inventory_actions.rs`); our
inventory only knows the classic Favorites *folder* (folder-icon
special-case in `inventory.rs`). This task is distinct from the
favorites-bar landmarks folder ([[viewer-navigation-favorites-bars]],
deferred) and from Firestorm's wearable-favorites floater
([[viewer-wearable-favorites]]).

Scope: read/write the AISv3 favorite field, the add/remove context
entries on items and folders, the star/tint row decoration (rides on
[[viewer-inventory-row-decorations]]), the display-style settings, the
outfit-favorite entries and favorites-to-top sorts on the outfit
gallery/list surfaces (which otherwise belong to
[[viewer-outfit-editor]]), and the Wearing-tab entries.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_inventory.xml`,
`menu_outfit_gear.xml`, `menu_outfit_tab.xml`, `menu_wearing_tab.xml`,
`floater_inventory_settings.xml`,
`indra/newview/llinventorymodel.cpp` (favorites),
`indra/newview/llaisapi.cpp`.
