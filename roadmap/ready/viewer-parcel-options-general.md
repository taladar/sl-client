---
id: viewer-parcel-options-general
title: About Land floater — general / covenant / objects
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-parcel-options
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The "About Land" floater, first half: view and edit parcel **general** info
(name, description, owner / group, area, sale state), the **covenant** tab, and
the **objects** tab (object counts, owners, return). This is the floater shell
plus the tabs that read and write parcel identity and land use.

Include the lightweight read-only **Location Profile** ("About this
location", World ▸ Location Profile / `World.PlaceProfile`) panel: the
place-profile view of the same parcel data without edit affordances
(main-menu survey 2026-07-23).

Reference (Firestorm, read-only): `llfloaterland`, `llpanelland`; the
`ParcelPropertiesUpdate` message.

Builds on: `protocol-13` parcel — note the known reality that rich parcel /
region data arrives over the CAPS event queue, not UDP.

Deps: [[viewer-ui-widget-scaffold]].

Note (2026-07-22): this floater is **subject-bound** — it opens on a
particular subject rather than persistent app state — so exempt it from
floater persistence (`floater_persist::FloaterPersistExempt` on the root,
as the avatar profile and item previews do): no restored rectangle, no
restored "open".
