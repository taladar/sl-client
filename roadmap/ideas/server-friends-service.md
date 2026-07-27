---
id: server-friends-service
title: Friends service — relationships, rights, online fan-out
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

Friendship storage and its live behaviour:

- the relationship table with the per-direction rights bits (see online
  status, see on map, modify objects) and their update flow
  (`GrantUserRights`);
- offer/accept/terminate transactions (the offer itself rides the
  message-routing service; the state change lands here);
- **online/offline notification fan-out**: when presence flips, every
  online friend's simulator gets `OnlineNotification` /
  `OfflineNotification` — the fan-out that makes presence and friends
  inseparable in practice;
- the login response's buddy list comes from here.
