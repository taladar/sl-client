---
id: viewer-nearby-voice-moderation
title: Moderate nearby (spatial) voice
topic: viewer
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
blocked_by: [viewer-voice-audio]
refs: [protocol-reference-capabilities, viewer-voice-controls]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life grants `SpatialVoiceModerationRequest`. A moderator — the parcel
owner with the session-moderator power on a voice-restricted parcel, otherwise
an estate manager — POSTs `{operand:"mute"|"unmute", agent_id}` to the
avatar's region, or `{operand:"mute_all"|"unmute_all"}` to the agent's own.
The muted resident gets `NearbyVoiceMutedByModerator` and loses push-to-talk.
WebRTC regions only.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
