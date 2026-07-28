---
id: server-estate-service
title: Estate service — cross-region estate state
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [server-simulator-core]
---

Context: [context/server.md](../context/server.md).

Estates group regions under one owner/manager set with shared policy,
so the state must live above any single simulator: estate records
(owner, managers, allowed/banned lists, covenant asset), the
region↔estate mapping, and the estate-wide actions (estate-wide ban,
message-all-regions, telehub policy).

Per-region *enforcement* stays in [[server-simulator-core]]; this
service is the shared source of truth simulators consult and estate
tools (`EstateOwnerMessage`) mutate. OpenSim's EstateService is the
reference shape.
