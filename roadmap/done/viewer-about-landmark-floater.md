---
id: viewer-about-landmark-floater
title: About Landmark floater — full detail view
topic: viewer
status: done
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

Shipped 2026-08-13 (about_landmark module):

- New standalone `about_landmark.rs` floater (`about-landmark`), opened
  by both the un-greyed context-menu **About Landmark** entry and by
  **Open** on a landmark (`inventory_properties` forwards; its old
  minimal inline preview — `preview-landmark`, `LandmarkText`,
  `ingest_preview_assets` — is deleted).
- Resolve chain, all pre-existing protocol: `FetchAsset` (landmark) →
  `parse_landmark` → `RequestRemoteParcelId` (the `RemoteParcelRequest`
  cap; first viewer consumer) → `RemoteParcelId` → `RequestParcelInfo`
  → `ParcelDetails`. The reply fills region name + coords (the
  landmark's own local position, reference behaviour), parcel name /
  description / maturity (0x2 adult, 0x1 mature) / owner (agent or
  group name via the shared caches, requested when missing) / traffic /
  area, the snapshot (shared texture pipeline, boosted), and the
  maps-URL SLURL (coords clamped to the 256 m grid) with a Copy button
  (`ViewerClipboard`).
- `RemoteParcelId` carries no correlation, so the floater keeps a
  single deadline-guarded await slot (10 s); on timeout the parcel row
  shows "(parcel details unavailable)" while the asset-derived rows,
  Teleport and editing keep working.
- Item title / notes edit in place (owner only), committed on Enter via
  the shared `send_item_update` (`UpdateInventoryItem`); creator /
  acquired shown like the properties floater.
- Unit tests: maturity / group-owned flag decode, SLURL formatting +
  clamping, region-line fallback (UUID before the name resolves).
