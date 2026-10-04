---
id: protocol-cap-dispatch-region-info
title: Write region settings over the DispatchRegionInfo capability
topic: protocol
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-region-options-estate]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants `DispatchRegionInfo` (OpenSim does not). Firestorm's region
General tab POSTs `{block_terraform, block_fly, block_fly_over, allow_damage,
allow_land_resell, agent_limit, prim_bonus, sim_access, restrict_pushobject,
allow_parcel_changes, block_parcel_search}`, falling back to the nine-string
`EstateOwnerMessage` `setregioninfo` — which has no `block_fly_over` or
`block_parcel_search` at all.

Our client sends `setregioninfo` (`Command::SetRegionInfo`, both runtimes).
Switch to the capability where granted, add the two flags to the command and
to the About Region controls (they only work on a grid granting the
capability).

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
