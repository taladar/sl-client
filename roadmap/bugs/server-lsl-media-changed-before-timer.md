---
id: server-lsl-media-changed-before-timer
title: A changed raised before a forced delay runs after the timer that fell due in it
topic: server
status: bugs
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
