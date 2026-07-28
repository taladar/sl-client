---
id: server-presence-service
title: Presence service — who is online, and where
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

Grid-wide session state: which agents are online, which region/simulator
each is on, and the session/secure-session ids that let other services
validate a request as coming from a live session.

- Written by login (session start), simulators (region changes), and
  logout/timeout reaping (a crashed simulator must not strand "online"
  ghosts — the login server's already-online handling depends on this).
- Read by the message-routing service (where to deliver an IM), the
  friends service (online-status fan-out), the map (agent counts /
  MapItem agent locations), and "presence" login failures.

OpenSim's PresenceService is the reference shape; SL treats presence as
part of its closed backbone, so behaviour is inferred from the client
protocol.
