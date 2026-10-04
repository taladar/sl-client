---
id: parcel-properties-update-via-udp-poisons-opensim
title: Parcel updates go over UDP, not the ParcelPropertiesUpdate CAP
topic: protocol
status: bugs
origin: found while live-testing viewer-menu-touch-object (2026-09-27)
refs: [viewer-parcel-config-missing-writes]
---

Context: [context/protocol.md](../context/protocol.md).

`Session`'s parcel edit (`circuit.rs` `send_parcel_properties_update`) only
knows the UDP `ParcelPropertiesUpdate`. The reference
(`LLViewerParcelMgr::sendParcelPropertiesUpdate`) posts the LLSD body
(`LLParcel::packMessage`) to the region's `ParcelPropertiesUpdate`
capability whenever the region grants it. It falls back to UDP only when
the region has no such capability.

This is a live bug on OpenSim, not only a CAPS-preference gap. The UDP
message has no `MediaType` field, so OpenSim's `LLClientView` leaves
`LandUpdateArgs.MediaType` null, and `LandObject.UpdateLandProperties`
copies it into the parcel. Its SQLite store declares `land.MediaType`
`NOT NULL`. From then on **every** commit in that region fails with
`Abort due to constraint violation — land.MediaType may not be NULL`:
the periodic land save, and also rezzing and attaching objects. One
`sl-repl` parcel edit on 2026-09-27 left the local grid unable to attach a
HUD until OpenSim was restarted. The attach failed on the grid, which
showed up in the viewer as "the HUD does not render".

Fix:

- Add the CAP path: an LLSD body with every field the reference's
  `packMessage` writes (`media_type` included), sent when the region
  granted `ParcelPropertiesUpdate`. Keep UDP as the fallback. Check that the
  capability is in the seed request and in the `granted.len()` tripwire.
- The LLSD body also carries fields the UDP block cannot, such as
  `see_avs`, the avatar-sound flags, `obscure_moap` and the media
  type/size/loop. That is most of what
  [[viewer-parcel-config-missing-writes]] is waiting on.
- Wire it through both runtimes. Add a fake-grid or loopback test that a
  parcel edit on a region granting the capability goes out as the CAP
  POST, not UDP.

## Capability (triage 2026-10-04)

Firestorm never hits this on OpenSim because it uses the
`ParcelPropertiesUpdate` capability, which both grids grant:
[[protocol-cap-parcel-properties-update]].

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.
