---
id: server-agent-transfer
title: Inter-simulator agent transfer — teleport, crossing, child agents
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [server-simulator-core, protocol-sim-udp-flows]
---

Context: [context/server.md](../context/server.md).

The simulator-to-simulator protocol that makes a multi-host grid feel
like one world — and the counterpart of the hardest client-side lessons
(cross-region teleport needs a child-agent circuit to the destination):

- **child agents**: a simulator asks its neighbours (via the grid
  service) to create child agents for an arriving avatar, keeps their
  throttles/interest updated (`ChildAgentUpdate`), and tears them down
  on distance;
- **teleport**: destination lookup, create-agent handshake with the
  destination simulator (agent circuit, seed cap, appearance/attachment
  state transfer), `TeleportFinish`/`EstablishAgentCommunication` to the
  client, source-side cleanup — the server side of the flow
  [[protocol-sim-udp-flows]] builds the wire machine for;
- **region crossing**: the same handoff under continuous movement
  (position/velocity handover, object/attachment migration, script
  state travelling with attachments), where latency budgets are
  tightest;
- failure handling: destination refuses/times out, capacity limits,
  banned/access-restricted destinations.

OpenSim's `EntityTransferModule` + inter-sim REST protocol is the
reference shape.
