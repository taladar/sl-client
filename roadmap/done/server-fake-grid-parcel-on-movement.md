---
id: server-fake-grid-parcel-on-movement
title: Fake grid — push the parcel an agent walks onto, and serve parcel environments
topic: server
status: done
origin: test-e2e-sweep-environment (2026-10-01)
points: 5
refs: [test-e2e-environment-parcel-layer, viewer-environment-personal-lighting]
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

## Measured (2026-10-05, [[gridspec-parcel-properties]])

`parcel-crossing` walks an avatar over a parcel line and back on both live
grids (`book/src/gridspec/land.md` § the pushes). Build the push to it:

- OpenSim pushes each crossing under sequence 0, about a second after the
  line. Second Life **counts its unsolicited pushes up for the session**: 0 on
  arrival, 1 on the first crossing, 2 on the next. The count is per session and
  belongs to this task's push: add it to `ParcelPolicy` and use it for every
  unsolicited push on the Second Life flavour (the arrival stays 0), then add
  the fake flavours to `parcel-crossing`'s grids so its `CROSSING_PUSH`
  constant holds them.
- Result `Single`, no snap, on both.

## Measured (2026-10-05, [[gridspec-environment]])

`book/src/gridspec/environment.md`. For the parcel environments this task
serves:

- OpenSim stores a parcel environment whatever the estate says, and **serves**
  it only while the estate allows parcel environments
  (`RegionAllowEnvironmentOverride`); otherwise a read answers `is_default`.
  The `FakeOpensim` flavour serves it regardless today. Second Life is
  unmeasured.
- A parcel with no environment of its own already answers `is_default` with no
  day on both flavours, and `environment` holds them to it.

## As built (2026-11-06)

- **The push**: the movement controller ([[server-world-agent-movement]])
  checks the parcel under the avatar every tick and pushes a new one to the
  agent's session, under the unsolicited 0 on OpenSim and under a count kept
  per session on Second Life (`ParcelPolicy::counts_unsolicited_pushes`; the
  arrival stays 0). A teleport within the region that lands on another
  parcel is pushed the same way. OpenSim's second of delay is not imitated:
  the push goes out in the step the avatar is over the line.
- **Parcel environments**: `RegionFixture::give_parcel_environment` serves an
  environment for one parcel and puts its version on the parcel's record;
  `SceneFixtures::allow_parcel_environments` sets the estate's
  `ALLOW_ENVIRONMENT_OVERRIDE`, which every parcel record then carries as
  `region_allow_environment_override` (an estate change by an estate manager
  re-stamps them). The OpenSim flavour serves a parcel's environment only
  while the flag is set — read per request, so a change mid-session counts —
  and answers `is_default` otherwise
  (`ImitatedGrid::parcel_environments_need_estate`,
  `SimSession::set_serves_parcel_environments`). A parcel without an
  environment says version -1, as both grids do.
- **Teeth**: `walking_over_a_parcel_line_pushes_each_parcel` walks a client
  over a line and back on each flavour (0, 0 against 1, 2), and
  `a_parcel_environment_is_served_while_the_estate_allows_it` reads a
  parcel's environment with the estate allowing it and not.

Not done: `parcel-crossing` on the fake flavours. It divides its line off the
region, which the fake grid cannot do yet; noted in
[[server-fake-grid-parcel-divide-join]].
