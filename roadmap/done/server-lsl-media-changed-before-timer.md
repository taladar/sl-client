---
id: server-lsl-media-changed-before-timer
title: A changed raised before a forced delay runs after the timer that fell due in it
topic: server
status: done
origin: server-lsl-lib-prim-state (2026-11-04)
refs: [server-lsl-lib-prim-state, server-lsl-state-and-events]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`prim/changed` calls each setter from a repeating one-second timer and counts
the `changed` events until the next timer. `llSetPrimMediaParams` and
`llClearPrimMedia` cost the script a second (the library table's forced
delay), in which that timer falls due. On a Second-Life-flavoured region the
engine queues a `changed` behind every other kind of event raised in the same
sleep (`events/order`), so the timer runs first and the media setter's
`CHANGED_MEDIA` is counted a window late; aditi counted it in its own window.
OpenSim's queue is first in, first out, and the replay agrees there.

## What to find out

Which of the measured rules does not hold here: whether Second Life's
forced delay for the media functions is a second at all (the table says so,
no probe timed it), whether a timer due during a forced delay (not an
`llSleep`) is queued then or when the script wakes, or whether the `changed`
rule of `events/order` is about events raised while the script slept, not
before. A probe timing the media setters and the timer around them on aditi.

## Done when

The engine orders them as aditi does and the `prim/changed` replay's
deviation goes.

## Resolution (2026-11-05)

None of the three candidate rules was the one: a probe of its own,
`events/timer-reset`, run on aditi and both OpenSim engines, measured the
media setters at a second each (`llGetPrimMediaParams` too), a timer due in
an `llSleep` and in a forced delay alike, and the `changed` rule of
`events/order` holding in both. What `prim/changed` hit is where the delay
was: in the timer's **own** handler, and on Second Life a timer does not
fall due while its own handler runs. One that came due meanwhile is raised a
frame or two after the handler ends, behind what the handler raised, and the
probe's `llSetTimerEvent(1.0)` at the end of the handler replaced it — so
the `changed` ran in its own window. Busy in any other handler the timer is
queued as it falls due, and kept by a new interval.

The runtime now does as each grid did:

- Second Life: `timer_due` holds a timer back while its handler is the body
  in progress (the old `(17, "timer 3")` unit expectation becomes 18).
- `llSetTimerEvent(0)` takes a `timer` still queued on Second Life and
  YEngine (OpenSim's `CancelScriptEvent`), not on XEngine — what
  `time/timer-rearm` had measured on 2026-10-28 and nothing implemented.
- XEngine takes the six media functions' forced delay before the call
  (`ScriptSleep` first in `LSL_Api`), so their `changed` follows a timer due
  in the delay; a body in progress keeps that it has slept first, which the
  saved-script form carries (version 5). XEngine's timers are posted after
  the tick's scripts ran (its async command pass), so a call held back by
  its delay wins a tie with the timer. The library's "XEngine raises nothing
  for a set and two for a clear" was that delay, and goes.
- OpenSim raises no `CHANGED_MEDIA` for a clear of a prim that never had
  media (`Prim::has_media_list`); Second Life does.

`prim/changed` replays without a deviation on all three flavours, and the
new `vm/tests/timers.rs` replays `events/timer-reset`. Gridspec: "A timer
that falls due while the script is busy".
