---
id: server-lsl-scripted-teleport-unmodelled
title: The fake grid ignores llTeleportAgent and llTeleportAgentGlobalCoords
topic: server
status: bugs
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [server-lsl-lib-avatar-control, server-lsl-lib-experience,
  test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`RegionHost::teleport_agent` (`sl-fake-grid/src/scripts.rs`) refuses every
scripted teleport as unmodelled, so `experience/powers` never moves an avatar
on either fake flavour. OpenSim lands the owner and a granting non-owner at the
aim. Second Life moves the owner alone, and routes it through the parcel's
landing point or telehub (`book/src/gridspec/lsl.md`, the measurements of
[[server-lsl-lib-experience]]). [[server-lsl-lib-avatar-control]] is done and
says a scripted teleport goes through the ordinary teleport phases, which the
fake grid's host does not do.

## Fix

Drive the grid's teleport machinery (`crate::teleport`, as
`FakeGrid::teleport_agent` uses it) from a script's teleport: a local
destination, a landmark, global coordinates and home, with Second Life's
landing point routing. `experience/powers` then agrees on both flavours, and
its entries in `probes/accepted.toml` go.
