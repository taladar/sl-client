---
id: aditi-3
title: Unknown CAPS event AgentStateUpdate
topic: aditi
status: done
origin: KNOWN_ISSUES_ADITI.md — issue 3 (planned)
refs: [missing-eq-batch-1]
---

Context: [context/aditi-issues.md](../context/aditi-issues.md).

One `UnknownCapsEvent message=AgentStateUpdate` warning was logged shortly after
login. This is a CAPS **EventQueue** push event (not an LLUDP message) that the
client does not yet recognize. Its body is `{ "can_modify_navmesh": bool }` —
the pathfinding flag for whether the agent may rebake this region's navmesh
(Firestorm `llpathfindingmanager.cpp`); SL-only (OpenSim never sends it).

**Status — planned.** Investigating this revealed a *second* coverage axis
parallel to the LLUDP gap: several CAPS EventQueue push events are unhandled
(`AgentStateUpdate`, `NavMeshStatusUpdate`, `AgentDropGroup`,
`DisplayNameUpdate`, `SetDisplayNameReply`, `WindLightRefresh`,
`SimConsoleResponse`, `RequiredVoiceVersion`, `OpenRegionInfo`; the
`ChatterBox*` session events belong to the `chat` topic). These are now
catalogued and batched under the CAPS EventQueue gap in the `missing` topic;
`AgentStateUpdate` is EQ batch 1 ([[missing-eq-batch-1]]), which closes
this issue.

## Resolution (done)

Closed by [[missing-eq-batch-1]] (now done). The CAPS EventQueue push decodes
into `Event::AgentStateUpdate { can_modify_navmesh }` via the
`"AgentStateUpdate"` arm in `handle_caps_event` (`sl-proto`
`session/methods.rs`, decoder in `session/conversions.rs`) — no more
`UnknownCapsEvent` fallthrough; covered by `sl-proto/tests/lifecycle.rs`.
