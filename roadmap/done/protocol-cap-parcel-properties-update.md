---
id: protocol-cap-parcel-properties-update
title: Edit parcels over the ParcelPropertiesUpdate capability
topic: protocol
status: done
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, parcel-properties-update-via-udp-poisons-opensim, viewer-parcel-config-missing-writes, gridspec-parcel-management]
---

Context: [context/protocol.md](../context/protocol.md).

Both grids grant `ParcelPropertiesUpdate`. Firestorm POSTs the parcel as LLSD
(`LLViewerParcelMgr::sendParcelPropertiesUpdate`,
`LLParcel::packMessage(LLSD&)`) and only falls back to the UDP message without
the capability. The LLSD body carries fields the UDP block cannot: `media_type`,
`see_avs`, `group_av_sounds`, `any_av_sounds`, `obscure_moap`.

**Do first**: the UDP path leaves `MediaType` NULL on OpenSim and breaks every
later database commit of that parcel
([[parcel-properties-update-via-udp-poisons-opensim]]); the capability path
fixes it.

Our client sends UDP `ParcelPropertiesUpdate` (`sl-proto` `update_parcel`,
both runtimes). Switch `Command::UpdateParcel` to the capability where granted,
including the fields the UDP form drops.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-parcel-management]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.

## Done (2026-10-05)

`Command::UpdateParcel` POSTs `build_parcel_properties_update_request` to the
capability where granted (both runtimes, failures reported as a capability
failure) and falls back to UDP otherwise; `ParcelUpdate` carries the media
block, link sharing, visibility flags and `obscure_moap` as `Option`s. The fake
grid serves the capability (`parse_parcel_properties_update_request`). Done
inside [[gridspec-parcel-management]].
