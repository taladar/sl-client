---
id: test-automation-quiet-after-teleport-reads-old-position
title: After a teleport or crossing, Quiet and the agent probe still hold the old region's position
topic: test
status: done
origin: found while fixing test-automation-quiet-before-own-avatar-arrives (2026-11-15)
refs: [test-automation-quiet-before-own-avatar-arrives]
---

Context: [context/automation.md](../context/automation.md).

## Observation

Since [[test-automation-quiet-before-own-avatar-arrives]], `Quiet` needs
`QuiescenceReadout::agent_present`, read from `AgentRegionPosition`. That
resource is filled by the own avatar's object updates (and a `TeleportLocal`)
in `status_bar.rs`'s `track_agent_position`, and **nothing clears it on a
region change**. So after a teleport or a crossing, until the destination's
first own-avatar update:

- `agent_present` is already true, so a `Quiet` wait right after the region
  name changes (as `tests/e2e_arrival.rs` does after both its teleport and its
  crossing) can hold before anything of the destination has arrived, the
  same gap the login had;
- the `Agent` probe's `position`, and the status bar's coordinates, are the
  old region's region-local coordinates shown under the new region's name.

## Why it was not fixed with the login case

Clearing the position on `SessionEvent::RegionChanged` is the obvious change,
but `RegionChanged` is pushed only once the handover completes, and the
destination of a crossing or a neighbour teleport is already a child circuit
that may have sent the own avatar's update **before** it. A standing avatar
then sends nothing more, so the position would stay `None` and `Quiet` would
never hold.

## To do

- Find, from a fake-grid crossing and a neighbour and a distant teleport, the
  order of the destination's own-avatar update and `RegionChanged`.
- Tag the position with the region it came from (the update's region handle,
  which the session knows per circuit), and count it present only when it is
  the current region's. That is right whatever the order, and it also stops
  the status bar showing the old coordinates.
- A unit test for each order, and the `e2e_arrival` teleport and crossing
  legs as the end-to-end check.

## Done (2026-11-19)

`AgentRegionPosition` now keeps the own avatar's latest position per region
whose update reported it (`Object::region_handle`), with the current region
beside it, and `position()` / `current()` read only the current region's.
`enter_region` drops every position but the new region's own, so a
destination's update that came before the region change counts from it,
and a region the agent returns to does not revive the position it held
before it left. The status bar's `track_agent_position` follows
`SlIdentity::region_handle` into it before folding the frame's updates, and
records a `TeleportLocal` against the current region. The `Agent` probe's
`position`, `Quiet`'s `agent_present`, the ground height under the agent
(now from the position's own region), the status bar's coordinates, the
About Land landing point and the experience profile's location all read it.

Order on the fake grid, from probe logging in `track_agent_position` over
`e2e_arrival`'s distant teleport and crossing: the region change lands
first and the destination's first own-avatar update in the **same frame**.
So the e2e legs (which now also assert that once quiet the agent stands at
the landing point, moved off the region centre, and just over the border)
pass with the fix and also with the old carried-over position, measured by
running them against a deliberately reverted `enter_region`; the two
orders are told apart by the unit tests: `world_state`'s four (a change
drops the old position, an early update counts after it, a return does not
revive one, the same region keeps and no region forgets), the status bar's
two system tests for each order, and the quiescence probe test now
entering a new region and reporting the agent absent until it reports.
