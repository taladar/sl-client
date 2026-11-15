---
id: test-automation-quiet-after-teleport-reads-old-position
title: After a teleport or crossing, Quiet and the agent probe still hold the old region's position
topic: test
status: bugs
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
