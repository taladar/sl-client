---
id: viewer-places-landmarks
title: Places floater — landmarks, create landmark, teleport history
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-inventory-folder-tree]
refs: [viewer-about-landmark-floater, viewer-world-map-floater, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

The Places floater — the landmark-centred navigation hub:

- **My Landmarks**: the inventory Landmarks branch (+ Favorites folder) as a
  dedicated tree with teleport-on-double-click, and the landmark actions
  (teleport, show on map, rename, delete, move between folders).
- **Create landmark**: the "Landmark This Place" flow — create the landmark
  asset for the current position (`CreateLandmarkForEvent` / inventory
  create path), pick folder + name inline. Also "Set Home to Here"
  (`SetStartLocationRequest`), which the reference keeps nearby.
- **Teleport history**: the session's (and persisted recent) teleport
  destinations with timestamps, teleport-back and show-on-map per row.
- A selected entry shows its detail via the About Landmark surface
  ([[viewer-about-landmark-floater]]).

Landmark asset decode (region handle + position from the asset blob) is
required for detail display and map jumps — small, belongs in `sl-asset`
alongside the other decoders.

Reference (Firestorm, read-only): `llpanelplaces`, `llpanellandmarks`,
`llpanelteleporthistory`, `llfloatercreatelandmark`,
`fsfloaterplacedetails`, `fsfloaterteleporthistory`.

Builds on: the inventory model, the teleport command/event surface
(`protocol-10`), the SLURL dispatcher ([[viewer-slurl-parse-dispatch]]).

## Parity-audit addendum (2026-08-19)

The parity audit adds World ▸ **Teleport Home** (`World.TeleportHome`)
— a menu action teleporting the own avatar home;
`teleport_via_landmark(nil)` already exists in
`sl-proto/src/session/methods.rs`, so this is a menu entry over an
existing protocol call, sitting next to the task's "Set Home to Here"
scope.

Scope additions from the parity audit: landmark/place **Copy SLurl**
and **Create Pick** from a place or landmark row (Create Pick jumps
into the
picks editor — picks editing itself exists via viewer-social-profiles;
the from-a-place entry point rides here). Also the teleport-history gear
details (menu_teleport_history_gear.xml family): **Clear Teleport
History**, the timezone choice (UTC / SLT / Local) for history entries,
and the show position / date column toggles.
