---
id: server-lsl-event-raise-timing
title: Raise each event when each grid raises it, not when the tick is over
topic: server
status: done
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

## Done (2026-11-08)

- **Measured first**: the new `events/latency` probe (an echo script, a
  talker in prim B, and nine kinds of trial) times a link message to itself
  and to an echo, chat to a talker and back, a sweep's answer, a setter's
  `changed`, an answer seen while busy polling, and races a sweep's answer
  against a timer and a link message. Recorded on aditi, both OpenSim engines
  and both fake flavours. Second Life runs a link message a frame after it
  was sent, a setter's `changed` and a sweep's answer two frames after the
  call, and a line said to a script at once; OpenSim runs the first three in
  the call and hands a line over at its next async command pass (every
  100 ms).
- **Queued at the raise, ready later**: `Arrival::ready` holds a queued event
  back until its tick, so a state change or reset still drops it, and the
  script runs the first queued event that is ready (`start_next_event(now)`).
  The engine itself now raises chat to scripts (`Raised::Heard`, from
  `Host::say` / `say_to` returning the `Overheard` recipients), a setter's
  `changed` (`Raised::Changed`; `Host::set_prim` lost its `changed`
  parameter) and OpenSim's in-call sweep (`Raised::Sweep`).
- **OpenSim's leftover share**: a script that an event reached after its turn
  in the tick runs it on what is left of its share, as an engine thread
  would, so an echo answers in the tick it was asked.
- **Held to the grids**: `vm/tests/latency.rs` (seven tests, per flavour);
  `corpus/events/changed-from-a-setter` now has per-flavour expected files;
  `events/latency` joined the differential suite. `events/order` and
  `events/latency` agree on both grid pairs, and the three accepted entries
  are gone.
- **Residuals**, documented in the book: on fake-opensim a busy poller sees
  an answer a tick late (its share is spent), and the chat round trip reads
  0.18 s against the live 0.1 s, a phase effect of the async pass.
- **Aditi is not fixed in round 4**: one of three aditi runs on server
  2026-11-05 printed `events/order` round 4 in the order raised (`listen,
  link5, sensor, changed2, timer, touch_start, touch_end`); the other two
  ranked it as recorded. The fake grid keeps the ranked order.
