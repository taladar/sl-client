---
id: protocol-cap-dispatch-region-info
title: Write region settings over the DispatchRegionInfo capability
topic: protocol
status: done
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

## Done together with [[gridspec-region-info]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.

## Done (2026-10-17)

Done in [[gridspec-region-info]]: `Command::SetRegionInfo` POSTs the
capability where granted (both runtimes), `RegionInfoUpdate` gained
`block_fly_over` and `block_parcel_search`, the fake grid serves it on
`FakeSl`, and the About Region Region tab has both boxes, greyed where the
capability is not granted.
