---
id: viewer-parcel-options-general
title: About Land floater — general / covenant / objects
topic: viewer
status: done
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

## Done (2026-07-28)

Implemented in `sl-client-bevy-viewer/src/about_land.rs` as the whole nine-tab
`About Land` floater (this task plus [[viewer-parcel-options-access-media]] and
the Options / Experiences / Environment tabs) — the user directed the full
floater in one pass. Highlights:

- **General** — name / description edit, parcel id, land type + rating (from the
  region), owner / group resolved to **names and clickable** (open the avatar /
  group profile), area, claim date, traffic (dwell), sale state; **Apply** →
  `Command::UpdateParcel`. The read-only "Place Profile" variant is the same
  floater opened with edit affordances suppressed (World ▸ Place Profile).
- **Covenant** — estate name / owner, the covenant notecard text (fetched by id)
  - timestamp, region name / type / rating, resale / subdivide clauses.
- **Objects** — the prim accounting, auto-return, and the object-owners
  **table** (bounded, scrolling) with Refresh.
- **Opened from**: the top-bar location read-out (current parcel), the land pie
  (the clicked parcel — resolved by asking the sim for the parcel at the point),
  and World ▸ About Land / Place Profile.

Built **once + updated in place** (no despawn); variable lists use the table
widget; disabled controls honour `InteractionDisabled` — the shared widgets
(combo, text input, radio, texture / colour swatches) were fixed to respect it
per-observer so a disabled control greys and refuses input while a disabled list
still scrolls.

Follow-ups (own tasks): [[viewer-neighbor-region-parcels]] (About Land on a
neighbour region's parcel), [[viewer-parcel-config-missing-writes]] (the
read-only controls that lack a protocol write path). Land buy / sell / abandon /
deed / buy-pass remain their own land-holdings tasks.
