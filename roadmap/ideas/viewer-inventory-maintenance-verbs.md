---
id: viewer-inventory-maintenance-verbs
title: Firestorm inventory power / maintenance verbs
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-inventory-context-actions, viewer-object-rezzing]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm-specific batch of low-level inventory maintenance verbs we
carry nowhere: **Reload Folder** (re-fetch a folder from the server,
`reload_folder`), **Move to Default Folder** (re-file an item by asset
type, `move_to_default_folder`), **Move to Lost And Found**, **Delete
System Folder**, **Create Folder From Selected**, **Ungroup Folder
Items** (`ungroup_folder_items`), folder-level **Copy UUID**, **Change
Type** (re-type a wearable, e.g. gloves to jacket, `changeType`), the
verb **Restore to Last Position** (rez a taken object back at its
recorded world position — `restoreToWorld`, dangerous and
confirmation-gated in Firestorm), and the gear toggle "Add
objects/clothes on double click".

Individually each verb is tiny; collectively they are the FS power-user
maintenance kit, and none is covered by another task. Our item and
folder context menus live in
`sl-client-bevy-viewer/src/inventory_actions.rs`
([[viewer-inventory-context-actions]] done); Restore to Last Position
additionally needs the rez path from [[viewer-object-rezzing]] plus the
stored last-position metadata.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_inventory.xml`,
`menu_inventory_gear_default.xml`,
`indra/newview/llinventorybridge.cpp` (`reload_folder`,
`move_to_default_folder`, `ungroup_folder_items`, `restoreToWorld`,
`changeType`).
