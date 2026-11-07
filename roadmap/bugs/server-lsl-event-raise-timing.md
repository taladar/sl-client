---
id: server-lsl-event-raise-timing
title: Raise each event when each grid raises it, not when the tick is over
topic: server
status: bugs
origin: test-lsl-differential-opensim (2026-11-07)
refs: [test-lsl-differential-opensim, server-fake-grid-script-engine-wiring,
  server-lsl-lib-detection-sensors]
---

Context: [context/lsl.md](../context/lsl.md).

`sl-conformance differential events/order` (round 2 and round 4 of the
probe) shows the fake grid putting events raised in one sleep in another
order than the grids did, not because the queue ranks them otherwise but
because it **raises** them at another moment:

- **OpenSim** raises `CHANGED_COLOR` inside `llSetColor`
  (`SceneObjectPart.TriggerScriptChangedEvent` posts it during the call),
  so a colour change and then a link message arrive `changed2,link2`; the
  fake grid applies the setter's `Effect::Changed` after the tick, behind
  the link message the helper sent in the same handler (`link2,changed2`).
- **OpenSim** answers `llSensor` inside the call (`SensorRepeat.SenseOnce`
  runs the sweep at once), so its answer comes first in round 4; the fake
  grid sweeps at the next tick (`ScriptCtx::sense_once`).
- **Second Life** delivers the talker's line before the helper's link
  message, and the `llSensor` answer after the 0.3 s timer (`listen, link5,
  timer, sensor`); the fake grid has `link5, sensor, listen, timer` — a
  link message reaches its target in the tick it was sent, a sweep is
  answered at the next one.

Wanted: an event raised by another script's call (a `changed` from a
setter, a link message) posted at the point in that call where its grid
posts it, and each flavour's latency for a sweep's answer, chat and link
messages — measured first where the probe does not pin it (a Second Life
probe of link-message versus chat latency).

The rank within one handler's events and their batching are right since
2026-11-07 (`a_changed_waits_behind_its_own_handlers_events_alone`); this is
about the moment each event is raised.

Acceptance: `events/order` agrees between each grid and its fake flavour,
with the three `events/order` entries of `sl-lsl-runtime/probes/accepted.toml`
removed.
