---
id: viewer-inventory-link-tools
title: Inventory link tools — find original, find all links, visibility
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-inventory-link-replace, viewer-inventory-floater-menus,
  viewer-inventory-cof-maintenance]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's link toolset beyond Replace Links
([[viewer-inventory-link-replace]]): **Find Original** (jump from a link
row to the item it points at), **Open Original**, **Find All Links**
(filter the view to every link pointing at the selected item), the gear
menu's link-visibility trio (**Show Links / Show Only Links / Hide
Links**), and Firestorm's **Cleanup broken Links** (delete links whose
target item is gone).

Our inventory gear menu ships all of these greyed as UNIMPLEMENTED
(`sl-client-bevy-viewer/src/inventory.rs` INVENTORY_GEAR_MENU, noted as
such in [[viewer-inventory-floater-menus]]), and the item context menu
in `inventory_actions.rs` has no Find Original / Find All Links entries
at all. The held inventory model already indexes links — the COF
maintenance pass walks them ([[viewer-inventory-cof-maintenance]]) — so
this is view-filter and selection work on the existing tree plus a
delete pass for broken links.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_inventory.xml`,
`menu_inventory_gear_default.xml`,
`indra/newview/llinventorybridge.cpp` ("find_links" /
"find_original"), `indra/newview/llpanelmaininventory.cpp`.
