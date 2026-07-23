---
id: viewer-minimap-menu-land-items
title: Minimap context menu — About Land / Place Profile / World Map
topic: viewer
status: blocked
origin: split from viewer-minimap-interactions (2026-07-23)
blocked_by: [viewer-parcel-options-general, viewer-world-map-floater]
refs: [viewer-minimap-interactions, viewer-rlv-enforce-info-hiding]
---

Context: [context/viewer.md](../context/viewer.md).

The minimap context menu's location entries, waiting on their floaters:

- **About Land** — select the parcel at the right-click's world position
  and open the About Land floater
  ([[viewer-parcel-options-general]]); enabled only over a valid
  parcel (the reference re-checks the hover parcel while the menu is
  open).
- **Place Profile** — the place-details view for the clicked position.
- **World Map** — open the world-map floater
  ([[viewer-world-map-floater]]); once it exists, the double-click
  action 1 ("open world map") also stops falling back to
  beacon-only, and both hand the clicked position to the shared
  tracking (`MapTracking` is already the shared resource).

RLV can disable the location / world-map items
([[viewer-rlv-enforce-info-hiding]]).
