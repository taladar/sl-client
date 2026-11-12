---
id: server-fake-grid-script-animation-source-is-the-avatar
title: An animation a script starts is stated as the avatar's, not the prim's
topic: server
status: bugs
origin: server-fake-grid-prim-sit-states-no-sit-animation (2026-11-12)
refs: [server-fake-grid-prim-sit-states-no-sit-animation,
  server-lsl-lib-avatar-control]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

The probes `avatar/animation` and `avatar/sit-animation` note each
`AvatarAnimation` with the source of every built-in in it. On aditi and on
OpenSim (both engines) what a script started is sourced from the script's
prim (`dance1@probe`, `sit@probe`). On both fake flavours the same entries
carry no `@probe`: `AnimationSet` keeps no source per entry and states the
agent itself as the source of everything started on top of the default
(`AnimationSet::playing`), whether a viewer or a script started it. The
runner's notes are not compared, so `compare-probes` does not report it.

## Wanted

An entry a script started is stated with its prim as its source, to the
animated avatar's own viewer and, where the grid names sources to others,
to the region's other viewers, per flavour as each grid states it (measure
OpenSim's `LLClientView.SendAnimations`, which lists the objects' entries,
against Second Life's statement to another viewer before modelling the
second).
