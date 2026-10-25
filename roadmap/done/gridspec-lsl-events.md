---
id: gridspec-lsl-events
title: The LSL event model on OpenSim, beside what aditi already showed
topic: gridspec
status: done
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

## Done (2026-10-25)

- **Discover.** Five probes, `sl-lsl-runtime/probes/events/`: `order`
  (link messages, colour changes, a sensor's answer, another prim's chat, a
  timer and a touch raised in two orders while the probe sleeps), `merging`
  (nine rounds of one kind raised several times), `collision` (a physical
  prim falling onto the probe's, solid, under volume detect, and while the
  script sleeps), `attach` (attached from the ground, dropped,
  `llAttachToAvatar`, taken and rezzed again) and `state-drops` (a repeating
  sensor, taken controls, permissions, targets and a listen opened in
  `state_exit` across a state change). All complete on aditi, YEngine and
  XEngine. The first XEngine run of `attach` looped to the deadline: XEngine
  resets a script on a take and a rez, so the probe now marks its first
  start in the object's description and prints `restarted`.
- **Runner.** Three drives, the owner's alone: `attach point=<n>`, `drop`
  and `rerez` (take into the Trash and rez the item back). Prim A's newest
  object update is kept, a new key it takes is the probe's from then on, and
  a line printed ahead of the update that names it is replayed rather than
  lost; a prim still worn at the end is detached instead of derezzed.
- **Document.** `book/src/gridspec/lsl.md` § Events, with the 2026-10-23
  queue, timer and state-change rows; the drives in
  `book/src/tools/lsl-probes.md`. The striking rows: OpenSim's queue is
  first in, first out across every type and nothing merges; on Second Life
  `changed` and touches raised during a sleep merge (five colours one
  `changed`, three touches one) and come after everything else, a colour
  set to itself still raises, a collision during a sleep is lost, targets
  survive a state change, and attaching raises no `changed` where OpenSim
  raises its own bit 32768.
- **Not measured: region start.** `CHANGED_REGION_START` and the order
  after a restart are handed to [[gridspec-lsl-persistence]], which restarts
  the region anyway: aditi needs estate rights, and the runner does not
  survive a restart.
- **Fake grid.** Large, as planned: the measured rows are in
  [[server-world-changed-raisers]], [[server-world-touch-and-grab]],
  [[server-world-collision-and-physics]], [[server-world-sit-and-attach]],
  [[server-lsl-lib-detection-sensors]], [[server-lsl-lib-comms]] and
  [[server-lsl-lib-avatar-control]]. `Coalescing`'s docs now say which rule
  is which grid's. No flavour knob yet: the fake grid runs no scripts, so
  one would have nothing to read it.
- **Viewer.** None needed: nothing of a queue reaches it, and a state change
  sends it nothing.
