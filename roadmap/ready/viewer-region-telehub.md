---
id: viewer-region-telehub
title: Telehub management floater
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-region-options-general, viewer-region-options-estate, api-g8]
---

Context: [context/viewer.md](../context/viewer.md).

The reference Region/Estate floater's General tab has a "Manage Telehub…"
button (`panel_region_general.xml`, `manage_telehub_btn`) that opens the
telehub floater (`llfloatertelehub.cpp`, `floater_telehub.xml`): connect
the region's telehub to the currently selected object, disconnect it, list
the telehub's spawn points, and add/remove spawn points (added at the
selected object's position), with in-world highlighting of the hub object
and each spawn position while the floater is open.

Our About Region floater (`sl-client-bevy-viewer/src/about_region.rs`) has
no telehub button and the viewer has no telehub UI at all, while the wire
side is complete and unused: sl-proto carries `RequestTelehubInfo`,
`ConnectTelehub`, `DisconnectTelehub` and `AddTelehubSpawnPoint`
(`sl-proto/src/command.rs`, all EstateOwnerMessage requests) plus the
`TelehubInfo` reply decode, all delivered by [[api-g8]]. Implementing this
means adding the Manage Telehub button to the Region tab (estate-owner
gated), a small floater listing the hub object and spawn points over
those commands, connect/disconnect wired to the current edit-tool
selection, and ideally beacon-style highlighting of hub/spawn positions.

Reference (Firestorm, read-only): `indra/newview/llfloatertelehub.cpp`,
`indra/newview/skins/default/xui/en/floater_telehub.xml`,
`indra/newview/skins/default/xui/en/panel_region_general.xml`.
