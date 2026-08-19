---
id: viewer-land-access-list-export-import
title: Access-list export / import / copy for parcel & estate lists
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-parcel-options-access-media, viewer-region-options-estate]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm adds Export… / Import… (file-backed) and Copy buttons to the
About Land Allowed/Banned lists and to all four estate access lists
(managers, allowed, allowed groups, banned), so land managers can move
ban lists between parcels and estates or share them.

Ours has none of these: the About Land access tab
([[viewer-parcel-options-access-media]], done;
`sl-client-bevy-viewer/src/about_land.rs`) and the About Region access
tab ([[viewer-region-options-estate]], in progress;
`sl-client-bevy-viewer/src/about_region.rs`) only add/remove single
entries. Implementing this means a file-format-compatible export/import
(Firestorm uses a simple name-per-line list format) plus a
copy-to-clipboard action on our table widgets, feeding the existing
`UpdateParcelAccessList` / `UpdateEstateAccess` write paths in bulk.

Reference (Firestorm, read-only): `indra/newview/llfloaterland.cpp`,
`indra/newview/llfloaterregioninfo.cpp` (FS export/import handlers),
`indra/newview/skins/default/xui/en/floater_about_land.xml`,
`indra/newview/skins/default/xui/en/panel_region_access.xml`.
