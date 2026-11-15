---
id: test-automation-quiet-before-own-avatar-arrives
title: The automation's Quiet holds before the own avatar has arrived
topic: test
status: bugs
origin: an automated aditi flight for viewer-agentupdate-cadence-effects (2026-11-15)
refs: [viewer-agentupdate-cadence-effects]
---

Context: [context/automation.md](../context/automation.md).

## Observation

`sl-viewer-ctl launch` printed `holding the viewer` at 11:38:40.135 on aditi.
The own avatar's first full object update came at 11:38:40.552, so the first
`sl-viewer-ctl agent --json` after "ready" had no `position` and a script
reading it failed with `KeyError: 'position'`. An earlier launch in the same
session took three minutes to settle, so how long this gap lasts depends on
what is streaming.

## Cause

Both `sl-viewer-ctl`'s `launch::start` and `sl-e2e`'s `stage::arrive` take
`/region/name` in the `Agent` probe as arrival and then wait for
`StateCondition::Quiet`. `QuiescenceReadout::is_quiet`
(`sl-automation-proto/src/probe.rs`) needs only `region_up` (a handshake),
no outstanding asset or build work, and no compiling pipelines. Between the
handshake and the first object update nothing is outstanding yet, so `Quiet`
can hold in that gap, before any object of the region, the own avatar
included, has come in.

## Fix

Make arrival mean the own avatar is in the world: a field on
`QuiescenceReadout` (read from what the `Agent` probe's `position` comes
from) that `is_quiet` requires. That fixes the launcher and every e2e stage
together. Stages that already wait for `/position` themselves (as
`e2e_terrain` does) are unaffected. A unit test on `is_quiet` and one on
the reader cover it. Then check whether any intermittent e2e failure that
reads the agent's position right after `arrive` goes away.
