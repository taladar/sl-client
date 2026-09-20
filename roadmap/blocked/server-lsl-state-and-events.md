---
id: server-lsl-state-and-events
title: The event and state machine — 35 events and the rules around them
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-lsl-vm-execution]
refs: [server-lsl-lib-detection-sensors, server-world-heartbeat]
---

Context: [context/lsl.md](../context/lsl.md).

A script is a state machine, and the rules around its queue are as
observable as the code in its handlers. OpenSim's
`bin/ScriptSyntax.xml` lists **35** events; Second Life has a handful
more (`experience_permissions`, `experience_permissions_denied`,
`linkset_data`, `path_update`, the game-control and damage pair).

The rules to implement and test, each of which content depends on:

- **Only handlers declared in the current state receive events.** An
  event with no handler in the current state is not queued and not
  delivered — it is dropped at the source, which matters because
  queueing it would cost memory and fire it on a later state change.
  This is why a script's *declared* handler set per state is part of the
  compiled program, not a run-time lookup.
- **The queue is bounded** (the reference's limit is 64) and overflow
  drops — except that some events never queue more than one instance:
  `timer` does not stack, `collision` and `touch` coalesce per tick with
  the detected list merged rather than appended twice. Measure the exact
  rule against the local OpenSim and record it in the test.
- **A state change** runs `state_exit` in the old state, **discards the
  entire event queue**, and runs `state_entry` in the new one. It also
  stops the timer, removes every listen, and releases
  `llTakeControls` — the cleanup content forgets and then relies on.
  `state` to the *same* state is a no-op, not a re-entry.
- **`default` is special**: it is where a reset lands and the only state
  a script may start in.
- **The detected block travels with the event.** `llDetected*` reads the
  block of the event currently being handled, and an event with no
  detection (a `timer`) leaves the previous block visible — a documented
  quirk worth pinning rather than "fixing".
- **`changed(integer change)`** with its `CHANGED_*` bitfield, raised
  from a dozen unrelated places (inventory, link, colour, shape, owner,
  region, region start, teleport, media, scale). Each raiser is a
  library or world task; the *fan-in* is here, as one function the rest
  call.
- **`on_rez(integer start_param)`** and `attach(key id)`, both fired by
  the grid rather than by another script, and `llGetStartParameter`
  reading the value `llRezObject` passed.

Reference: `Shared/Instance/ScriptInstance.cs` (`EventQueue`,
`m_stateEvents`, `PostEvent`) for the queue and filter behaviour.

Acceptance: a state change is shown to discard a queued event, stop the
timer and drop listens; an event with no handler in the current state
never reaches the queue; `changed` fires with the right bit for each of
at least five raisers; and a table test covers every one of the 35
events' parameter shapes against the syntax document.
