---
id: server-groups-service
title: Groups service — membership, roles, notices
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

The Groups V2-shaped backend: group records (charter, insignia, fees,
maturity), membership with roles/titles/powers (the 64-bit
`GroupPowers` mask semantics), invites and ejections, the active-group
and active-role state per agent, notice storage (with attachments), and
group land/deed hooks toward the estate/parcel side.

Serves the UDP group message family and the group caps; group chat and
notice *delivery* ride the message-routing service — this service owns
the membership answers ("who is in this group and may speak/receive").
The local OpenSim Groups V2 + MariaDB setup (see the memory on it) is
the working reference deployment.
