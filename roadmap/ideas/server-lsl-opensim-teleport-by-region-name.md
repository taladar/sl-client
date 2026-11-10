---
id: server-lsl-opensim-teleport-by-region-name
title: OpenSim's llTeleportAgent to a region named in place of a landmark
topic: server
status: ideas
origin: server-lsl-scripted-teleport-unmodelled (2026-11-10)
refs: [server-lsl-scripted-teleport-unmodelled]
---

Context: [context/lsl.md](../context/lsl.md).

OpenSim's `llTeleportAgent` (`LSL_Api.DoLLTeleport`) reads a destination that
names no landmark of the prim and is not an asset key as a **region name**,
and teleports the agent to `targetPos` there (`ViaLocation`). The runtime
treats every non-empty destination as a landmark name, and on OpenSim says
nothing when none is found, so the call does nothing.

Unmeasured on the local OpenSim. Measure it with a probe that names the
neighbouring region, then give `Destination` a region-by-name variant the
fake grid resolves with `GridCore::region_by_name`, on the OpenSim flavour
alone.
