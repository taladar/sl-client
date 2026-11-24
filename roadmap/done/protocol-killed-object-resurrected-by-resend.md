---
id: protocol-killed-object-resurrected-by-resend
title: A resent ObjectUpdate after KillObject may leave a ghost object
topic: protocol
status: done
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

## Fix (2026-11-24)

Fixed without waiting for a reproduction. The fix orders the update against
the kill and does not need a time guess.

- `WorldCache` remembers each kill for 60 s together with the sequence number
  of the packet that carried it.
- A simulator numbers packets as it sends them, and a resend keeps the original
  number. An update numbered below the kill was therefore sent while the object
  existed, and `Session::update_predates_kill` drops it.
- The same check filters the misses that a cached probe or a terse update would
  ask the region for again.
- An update numbered above the kill is the object really coming back, and it
  ends the record.
- When the two numbers cannot be ordered (a counter that wraps short of
  32 bits), the update is kept.

Test: `an_update_sent_before_its_objects_kill_is_dropped`
(`sl-proto/tests/lifecycle.rs`).
