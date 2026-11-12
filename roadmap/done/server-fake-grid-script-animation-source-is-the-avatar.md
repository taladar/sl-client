---
id: server-fake-grid-script-animation-source-is-the-avatar
title: An animation a script starts is stated as the avatar's, not the prim's
topic: server
status: done
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

## Done (2026-11-12)

What the grids state, from the committed probe transcripts and OpenSim's
source:

- to the animated avatar's own viewer, Second Life names every entry's
  source, the script's prim for what a script started (`dance1@probe`,
  `sit@probe` on aditi); OpenSim lists the entries an object started first
  and names their sources alone (`LLClientView.SendAnimations` splits the
  set into `withobjects` and the rest; `LSL_Api` starts an animation under
  `m_host.UUID`, the script's own prim);
- to another viewer, neither names the prim (`avatar/animation`'s `of=other`
  steps show `dance1` with no source on aditi and OpenSim), so that shape
  was already right.

`AnimationSet` now keeps, per entry, the prim whose script started it
(`Delivery::Animate` carries the calling script's prim).
`AnimationSet::own` builds the agent's own statement per flavour, and
`observed` builds what the others are told. Both return one `Statement`
(the former `Observed`). A start of something already playing keeps whose
it was, and a stop stops it whoever started it. Unit tests hold both
flavours' own shape and the others' shape. On both fake flavours the
`avatar/animation` and `avatar/sit-animation` runs now note `@probe` where
their grids do. The gridspec's script-animation rows record the fake grid
column.
