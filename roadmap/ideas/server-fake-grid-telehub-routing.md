---
id: server-fake-grid-telehub-routing
title: Fake grid — route teleports to a region's telehub and spawn points
topic: server
status: ideas
origin: server-lsl-scripted-teleport-unmodelled (2026-11-10)
refs: [server-lsl-scripted-teleport-unmodelled, gridspec-estate,
  server-fake-grid-teleport-edges]
---

Context: [context/server.md](../context/server.md).

The fake grid routes a teleport to a parcel's landing point on the Second Life
flavour since [[server-lsl-scripted-teleport-unmodelled]], but it keeps no
telehub: the estate's `telehub` messages are decoded (`estate.rs`) and nothing
stores the hub or its spawn points, and the world map shows none.

aditi's Mauve has a telehub, and `experience/powers` lands every scripted
teleport inside the region by one of its spawn points, 18 to 69 m from the aim,
and refuses a teleport that would bring the avatar no closer with `Could not
teleport closer to destination` (`book/src/gridspec/lsl.md`). OpenSim's
`ScenePresence.CheckLocalTPLandingPoint` spells out the rule it copied from
Second Life: the spawn point nearest the aim, or the region's centre with no
spawn points, unless the avatar already stands closer; and with
`AllowDirectTeleport` set, as on the local OpenSim, no routing at all.

## What to do

Keep a region's telehub (the object and its spawn points) from the estate
messages and a fixture, route a teleport through it in `teleport_session`
beside the landing point, and show it on the world map. A fake-sl region with a
telehub placed as Mauve's could then retire `experience/powers`' accepted
entry in `probes/accepted.toml`, which names this task.
