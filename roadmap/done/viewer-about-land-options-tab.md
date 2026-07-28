---
id: viewer-about-land-options-tab
title: About Land floater — Options tab
topic: viewer
status: done
origin: About Land floater build (2026-07-28); tab not covered by the general/access-media split
refs: [viewer-parcel-options-general, viewer-parcel-options-access-media]
---

Context: [context/viewer.md](../context/viewer.md).

The reference About Land floater's **Options** tab was not covered by either
half of the original split ([[viewer-parcel-options-general]] general/covenant/
objects, [[viewer-parcel-options-access-media]] access/media/sound), so it is
recorded here. Built as part of the same nine-tab floater
(`sl-client-bevy-viewer/src/about_land.rs`).

Contents (all editable via `ParcelUpdate` / `ParcelFlags`, gated on parcel
ownership): the "allow other Residents to" checkboxes (edit terrain; fly;
everyone / group build; everyone / group object entry; everyone / group run
scripts), the land options (safe / no-damage, no pushing, show in search,
moderate content), the search **category** combo, the **snapshot** texture
(picker), the **landing point** (Set to the agent's position / Clear), and the
**teleport routing** (landing-type) combo, then **Apply**.

Flag bits missing from `sl-wire`'s `ParcelFlags` were added for these tabs:
`ALLOW_GROUP_SCRIPTS` (1<<25), `SOUND_LOCAL` (1<<15), `MATURE_PUBLISH` (1<<18);
plus `RegionFlags::ALLOW_PARCEL_CHANGES` (1<<26) for the covenant subdivide
clause.
