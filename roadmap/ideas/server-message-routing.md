---
id: server-message-routing
title: Global message routing — IMs, offers, notices across hosts
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-caps-agent-comms]
---

Context: [context/server.md](../context/server.md).

The grid-wide delivery fabric for everything `ImprovedInstantMessage`
carries and its CAPS-era siblings — the piece that makes agents on
different simulators (and hosts) reachable:

- agent-to-agent IM: resolve the recipient via presence, forward to the
  owning simulator, deliver; **offline storage** when the recipient is
  away (the `ReadOfflineMsgs` cap / `RetrieveInstantMessages` backend),
  with the email-forwarding option SL offers;
- the offer dialogs riding IM: inventory offers (with the
  auto-accept/busy rules), friendship offers, teleport lures/requests,
  group invites — each with its accept/decline round-trip routed back;
- **group messaging**: group notices (store, fan-out to online members,
  offline delivery) and group chat session fan-out (the ChatterBox
  sessions of [[protocol-sim-caps-agent-comms]]) to every member's
  simulator;
- typing indicators, busy/DND responses, and the delivery-failure
  paths (muted, blocked, capped offline queue).

OpenSim splits this across an IM module + offline-IM service +
Groups V2 messaging; SL routes it through its closed backbone. A single
routing service with per-message-type policy is likely the cleaner
design.
