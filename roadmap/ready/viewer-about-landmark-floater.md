---
id: viewer-about-landmark-floater
title: About Landmark floater — full detail view
topic: viewer
status: ready
origin: user request (2026-07-22), noticed while reviewing the minimal
  Open preview shipped with viewer-inventory-open-and-properties
refs: [viewer-inventory-open-and-properties, viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **About Landmark** detail window, beyond the minimal
preview the inventory Open shipped (region **UUID** + local position +
Teleport): resolve and show the **region name** and global coordinates
(`RequestParcelInfo` / the map name lookup — both on the wire already),
the destination **parcel's name, description and snapshot** (the parcel
info reply carries them; the snapshot renders through the shared
texture pipeline), a **copyable SLURL**, and the item's own name /
notes editing. Un-greys the item context menu's "About Landmark" entry
(today the preview only opens via Open); "Show on Map" stays with
[[viewer-world-map-tracking-teleport]].

Reference (Firestorm, read-only): `llpanellandmarkinfo.cpp`,
`llfloatercreatelandmark.cpp` (the modern places/landmark panels).
