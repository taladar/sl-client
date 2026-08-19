---
id: viewer-inventory-qol-toggles
title: Inventory quality-of-life toggles
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-inventory-context-actions, viewer-inventory-search-filter,
       viewer-inventory-thumbnails]
---

Context: [context/viewer.md](../context/viewer.md).

Small inventory behaviour toggles the reference gates individually,
each a line or two in our shipped inventory UI
([[viewer-inventory-context-actions]] and
[[viewer-inventory-search-filter]] are done): double-click on clothing
or attachments *adds* instead of replaces/wears
(`FSDoubleClickAddInventoryClothing` /
`FSDoubleClickAddInventoryObjects`), allow drag-and-drop moving of
folder links (`FSEnableMovingFolderLinks`), keep an independent search
term per inventory tab (`FSSplitInventorySearchOverTabs`), skip the
confirmation nag when emptying trash (`FSDontNagWhenPurging`), and sort
the #Firestorm/#RLV-style special folders alongside system folders
(`FSSortFSFoldersToTop`). Cosmetic knobs in the same family:
folder-row height (`FSFolderViewItemHeight`) and inventory-thumbnail
hover tooltips with a delay — the latter naturally lands with
[[viewer-inventory-thumbnails]].

Reference (Firestorm, read-only): `indra/newview/llinventorybridge.cpp`
(double-click add vs wear, folder-link moves),
`indra/newview/skins/default/xui/en/panel_preferences_UI.xml`,
`indra/newview/app_settings/settings.xml` (named settings).
