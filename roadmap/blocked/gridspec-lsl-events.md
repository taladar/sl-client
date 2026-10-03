---
id: gridspec-lsl-events
title: The LSL event model on OpenSim, beside what aditi already showed
topic: gridspec
status: blocked
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
