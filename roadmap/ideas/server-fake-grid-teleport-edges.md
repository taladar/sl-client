---
id: server-fake-grid-teleport-edges
title: Fake grid — the teleport edges each grid was measured handling and the fake grid does not model
topic: server
status: ideas
origin: gridspec-teleport (2026-10-07)
refs: [gridspec-teleport, gridspec-neighbours-crossing, gridspec-estate]
---

Context: [context/server.md](../context/server.md).

`book/src/gridspec/teleport.md` has rows the fake grid answers "not
modelled". None is needed by a test today; each is a row in
`ImitatedGrid::teleport_policy` when one is.

- **A position past the region's edge under the agent's own handle.** OpenSim
  carries it out as a teleport into the neighbour the position lies in
  (`x` 300 arrives at `x` 44 next door); Second Life answers a
  `TeleportLocal` stating `x` 300. The fake grid takes the position as given.
- **OpenSim's `MapBlockReply` after a teleport to a void region**, marking
  that map cell empty (access 254).
- **A full region.** OpenSim refuses with `The region is full`; Second
  Life's answer is unmeasured (needs estate rights, [[gridspec-estate]]).
- **The source circuit after a teleport.** The fake grid sends a
  `DisableSimulator` to a distant source on both flavours. Neither grid sent
  one within 25 s of a teleport to a *neighbour*; for a distant source it is
  not established — the client drops that circuit on arrival and stops
  listening. Measure it with a session that keeps the source circuit open
  ([[gridspec-neighbours-crossing]] watches the same circuits), then give the
  flavours their answers.
- **A destination that never confirms the arrival.** The fake grid sends
  `timeout_tport` on both flavours; OpenSim's source says `Problems
  connecting to destination …`, and Second Life's answer is unmeasured.

## From gridspec-neighbours-crossing (2026-10-07)

Rows of *Neighbours and crossings* the fake grid answers "not modelled",
each a row in `ImitatedGrid::neighbour_policy` when a test needs one:

- **Second Life's repeats.** Each edge neighbour's `EnableSimulator` again
  every 60 s, the corner neighbour's `EstablishAgentCommunication` every
  5 s, all of them again on a draw-distance change — always the same seed.
  The client is held to ignoring them by a unit test of the session.
- **OpenSim's half second between neighbours**, and Second Life sending
  each `EstablishAgentCommunication` only once the client has opened that
  circuit.
- **What a crossing's arrival re-announces.** On Second Life the region
  walked into announces its other neighbours again, and every circuit is
  greeted again; the fake grid announces only what the agent holds no
  session in.
- **OpenSim's `AgentDataUpdate` ahead of a `CrossedRegion`.**
- **The source circuit after a teleport**, above, can now be watched: the
  session reports a retired child with `Event::NeighborRetired`.
