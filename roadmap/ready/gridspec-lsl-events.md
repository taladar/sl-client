---
id: gridspec-lsl-events
title: The LSL event model on OpenSim, beside what aditi already showed
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Aditi: queue 64, timer never stacks and survives a state change, detected block
cleared, `changed` merging; OpenSim unmeasured for all of it.

## Discover

The event probes on YEngine and XEngine; ordering across types in one frame;
collision / sensor / listen / link_message merging; rez / attach / region-start
ordering; what a state change drops.

## Document

`book/src/gridspec/lsl.md` § Events.

## Fake grid

Large — [[server-world-changed-raisers]]; flavour knobs in `ImitatedGrid`.

## Viewer

None directly.

## From gridspec-lsl-probe-corpus (2026-10-23)

OpenSim is measured for part of it now: the corpus's `events/*` probes, on
both engines (table in `book/src/tools/lsl-probes.md`, files under
`sl-lsl-runtime/probes/results/`). The queue holds 64 on YEngine and is
unbounded on XEngine (100 of 100). Neither engine stacks a timer. Both drop
the queue and the listens at a state change and keep the timer. Neither
runs `state_exit` / `state_entry` for `state default` in `default`. A
`state` in a function ends the handler there on both, where Second Life's
caller carries on. Outside its event `llDetectedKey` / `llDetectedName` are
empty strings and the face is −1 on both, where Second Life answers
`NULL_KEY` and face 0. A touch's `llDetectedType` is 1 against Second
Life's 3. `changed` merging and ordering across event types are not
probed yet.
