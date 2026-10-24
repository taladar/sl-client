---
id: gridspec-lsl-comms
title: LSL library behaviour on each grid — chat, listens, dialogs and link messages
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-comms]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

What an *avatar* is sent of each say function was measured by
[[gridspec-local-chat]] (`book/src/gridspec/chat.md` § A script's lines):
types, owner, name, place, reach, the debug channel, an empty and a long
text. What a *listen* hears is still this task's.

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

## From gridspec-lsl-live-differential-runner (2026-10-24)

A probe can now ask for a line of chat on a channel or a touch, from the
owner or a second avatar (`rec=waiting drive=chat|touch by=other`;
`book/src/tools/lsl-probes.md`). `events/other-avatar` measured on all three
engines: an open listener on 7 hears the owner and the other avatar alike
(the name is `llKey2Name`'s), a listener filtered to the owner does not hear
the other, and the other's touch reads `llDetectedType` 3 on Second Life and
1 on OpenSim; `llDetectedGroup` is the object's group against the toucher's
active group (both unset on aditi: 1).
