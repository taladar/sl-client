---
id: protocol-sim-caps-experiences
title: Server-side experience caps
topic: protocol
status: blocked
origin: user request (2026-07) — complete simulator protocol surface
points: 3
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The experience cap cluster, server side, over a small experience fixture
set: `GetExperienceInfo`, `FindExperienceByName`, `GetExperiences`,
`AgentExperiences`, the admin/creator/group experience lists,
`ExperiencePreferences`, `IsExperienceAdmin` / `IsExperienceContributor`,
`UpdateExperience`, `RegionExperiences`.

Inverse-pairing with the client-direction support in
`sl-wire/src/experience*`; verified against it in-memory.
