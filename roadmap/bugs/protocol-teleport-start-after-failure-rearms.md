---
id: protocol-teleport-start-after-failure-rearms
title: A TeleportStart that arrives after its own failure starts a teleport that never ends
topic: protocol
status: bugs
origin: gridspec-teleport (2026-10-07)
refs: [gridspec-teleport]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

Second Life sends a refused teleport's `TeleportStart` and progress lines
over UDP and its `TeleportFailed` over the event queue. The session takes a
`TeleportStart` it is not expecting as a teleport the grid decided on
(`enter_remote_teleport`), which is right for `llTeleportAgent` and a forced
teleport home. So when the two transports deliver out of order — failure
first, start second — the session ends the teleport on the failure and then
starts another on the late `TeleportStart`, which nothing ever finishes: it
sits in `Teleporting` until its own thirty-second timeout reports a second,
spurious failure.

Seen on the fake grid's Second Life flavour before it was made to hold the
failure behind the client's acknowledgement of the last UDP line
(`teleport-failed` on `fake-sl`: the trace was `failed` with no start). On
aditi the two were a tenth of a second apart and arrived in order every time,
so this needs a delayed datagram to happen there. The same reordering after a
`TeleportCancel` is by design and harmless: the grid's failure follows.

## To do

Remember that a client-requested teleport just ended in a failure (for a
short while, or until the next request), and do not treat a `TeleportStart`
carrying the same flags in that window as a teleport of the grid's own. The
reference viewer has the same exposure; check what it does.
