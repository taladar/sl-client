---
id: protocol-killed-object-resurrected-by-resend
title: A resent ObjectUpdate after KillObject may leave a ghost object
topic: protocol
status: bugs
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/protocol.md](../context/protocol.md).

Plausible and not reproduced. Both messages are UDP, so this is reordering by
loss and resend rather than two channels. OpenSim's resend path re-checks
deleted objects, so this could only happen on Second Life.

## Bad order

`KillObject` is handled, then a resent `ObjectUpdate` for the same object
arrives.

## Cause

`upsert_object` (`sl-proto/src/session/methods.rs`) keeps no record of recently
killed objects. Firestorm's guard sits behind `IGNORE_DEAD`, which
`llviewerobjectlist.cpp` does not define, so it is probably equally exposed.

## Fix

Keep a short-lived record of killed local ids per circuit and drop updates for
them, if the race is shown to occur.
