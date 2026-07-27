---
id: server-experience-service
title: Experience service — records, permissions, key-value store
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

The experiences backend: experience records (name, group, maturity,
enabled/suspended), per-agent experience permission grants (the
allow/block lists the client edits via the experience caps), per-region
enablement, and the **experience key-value store** LSL scripts use
(llReadKeyValue/llUpdateKeyValue etc. — a server-only surface with no
client capability, which is why the protocol topic excludes it; a real
grid still needs it, with quotas per experience).

Consulted by the script engine for experience-scoped permissions and by
the simulators for region gating.
