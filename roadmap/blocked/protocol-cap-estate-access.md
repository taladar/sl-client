---
id: protocol-cap-estate-access
title: Read the estate access lists over the EstateAccess capability
topic: protocol
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, gridspec-estate, viewer-region-options-estate]
blocked_by: [gridspec-estate]
---

Context: [context/protocol.md](../context/protocol.md).

Both grids grant `EstateAccess` (GET only). Firestorm reads the four lists from
it — `{AllowedAgents:[{id}], AllowedGroups:[{id}], BannedAgents:[{id,
banning_id, last_login_date, ban_date}], Managers:[{agent_id}]}` — and ignores
the payload of the UDP `setaccess` reply, re-fetching over the capability
instead. The capability adds who banned whom, when, and the banned resident's
last login (OpenSim sends `"na"` for that). Edits stay on UDP
`estateaccessdelta`, as in Firestorm.

Our client reads the lists from the UDP `EstateOwnerMessage` `setaccess`
replies. Fetch over the capability where granted and carry the extra ban
fields to the estate Access tab.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-estate]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.
