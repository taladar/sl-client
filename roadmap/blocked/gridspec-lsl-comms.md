---
id: gridspec-lsl-comms
title: LSL library behaviour on each grid — chat, listens, dialogs and link messages
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-comms]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Listen limit, `llDialog` button / label errors, `llTextBox`,
`llRegionSayTo` to objects, `llOwnerSay` off-region, `llInstantMessage` delay.
Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Chat, listens, dialogs and link messages.

## Fake grid

Large — [[server-lsl-lib-comms]].

## Viewer

Dialog and textbox floaters, error display.
