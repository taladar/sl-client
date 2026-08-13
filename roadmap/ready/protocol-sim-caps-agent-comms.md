---
id: protocol-sim-caps-agent-comms
title: Server-side agent-communication caps
topic: protocol
status: ready
origin: user request (2026-07) — complete simulator protocol surface
points: 5
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The agent-communication cluster, server side:

- `ChatSessionRequest` — the group-chat ChatterBox session lifecycle,
  including the `ChatterBoxInvitation` /
  `ChatterBoxSessionAgentListUpdates` event-queue events;
- `ReadOfflineMsgs`;
- `GetDisplayNames` (paired with the existing `DisplayNameUpdate` EQ
  helper);
- `AgentPreferences`;
- `SendUserReport` / `SendUserReportWithScreenshot`.

Inverse-pairing per the convention; verified against the client-direction
builders/parsers in-memory.
