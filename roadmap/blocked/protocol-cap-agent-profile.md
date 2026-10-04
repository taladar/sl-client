---
id: protocol-cap-agent-profile
title: Read and write profiles over the AgentProfile capability
topic: protocol
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, gridspec-profiles, viewer-profile-image-editing]
blocked_by: [gridspec-profiles]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants `AgentProfile`; when it is there Firestorm sends **no** UDP
profile request at all. GET `{cap}/{avatar_id}` answers `{id, sl_image_id,
fl_image_id, partner_id, sl_about_text, fl_about_text, member_since, hide_age,
customer_type, notes, online, allow_publish, identified, transacted,
charter_member|caption, groups:[{id,name,image_id}], picks:[{id,name}]}`; PUT
`{cap}/{agent_id}` writes a one-key partial map (`sl_about_text`,
`fl_about_text`, `sl_image_id`, `fl_image_id`, `notes`, `allow_publish`,
`hide_age`). Firestorm calls the UDP notes path deprecated: the capability
allows longer notes.

Our client uses the UDP `AvatarPropertiesRequest` / `Update`,
`AvatarNotesUpdate` and the `avatarnotesrequest` / `avatarpicksrequest`
generic messages. Use the capability where granted (one GET instead of four
requests), keep UDP for OpenSim, and add `hide_age` to the profile.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-profiles]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.
