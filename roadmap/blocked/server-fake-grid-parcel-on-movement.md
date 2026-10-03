---
id: server-fake-grid-parcel-on-movement
title: Fake grid — push the parcel an agent walks onto, and serve parcel environments
topic: server
status: blocked
origin: test-e2e-sweep-environment (2026-10-01)
points: 5
refs: [test-e2e-environment-parcel-layer, viewer-environment-personal-lighting]
blocked_by: [gridspec-environment, gridspec-parcel-properties]
---

Context: [context/testing.md](../context/testing.md).

The fake grid sends a `ParcelProperties` for the agent's parcel once, on
arrival (`world.rs`), and never again: an agent that walks over a parcel line
is still told it stands on the parcel it arrived on. A real simulator pushes
the new parcel when the agent crosses into it (OpenSim's
`LandManagementModule`, on the presence's significant movement, with the
unsolicited sequence id), and the viewer's agent parcel — what its parcel
environment layer, its parcel media and audio and its About Land key on — only
moves when it is told.

- On an `AgentUpdate` whose position lands on another parcel than the one last
  pushed, push that parcel's properties (unsolicited sequence id), as the
  arrival does. Per session: each agent is told about its own parcel.
- Let a scene give a parcel an environment of its own: the session's
  `ExtEnvironment` store already keys environments by parcel id, so this is a
  region-config or scenario field that seeds one, plus the region's
  `region_allow_environment_override` (`world.rs`, today always `false`) and
  the parcel's `parcel_environment_version`, which the viewer reads to decide
  whether to ask.
- A teeth test at the session level: walk an agent over a line, and the second
  parcel's properties arrive; walk back, and the first's.
