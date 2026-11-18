---
id: protocol-teleport-start-after-failure-rearms
title: A TeleportStart that arrives after its own failure starts a teleport that never ends
topic: protocol
status: done
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

## Resolution

An event-queue `TeleportFailed` that ends a teleport whose `TeleportStart` has
not arrived (`TeleportPhase::Requested::start_seen`) now opens a ten-second
window (`LATE_TELEPORT_START_WINDOW` in `sl-proto/src/session.rs`) in which a
`TeleportStart` arriving outside a teleport is logged at debug and ignored
rather than entering a remote teleport. The next request needs no reset: it
moves the session to `Teleporting`, where a start is always its own.

The window is deliberately that narrow. A first cut opened it on every
failure, and the full suite caught what that costs:
`a_scripted_teleport_routes_to_the_landing_point_on_second_life` refuses one
scripted teleport (no start, failure only) and runs another straight after,
whose start the window swallowed. A failure with no teleport in flight, one
after its start, and OpenSim's UDP failure (sent *instead of* a start) have no
straggler coming, so they open nothing.

The flags are **not** compared, unlike the suggestion above: in the
failure-first order no start has been seen to compare with, and the flags a
request elicits differ by grid (Second Life adds `WITHIN_REGION` or
`SET_LAST_TO_TARGET`, see the gridspec's teleport page). A teleport the grid
really does start inside the window is not lost: it ends in a
`TeleportFinish` or `TeleportLocal`, both honoured without a start; only its
`TeleportStarted` event is missed.

The reference viewer has the same exposure: `process_teleport_start` enters
`TELEPORT_START` from `TELEPORT_NONE` on any start, with no record of a
failure.

Tests in `sl-proto/tests/lifecycle.rs`: a late start and its progress line
start nothing and leave nothing to time out or cancel; a `TeleportFinish` in
the window is still honoured; a start after the window is the grid's own
teleport again; a failure with no teleport in flight or after its start
leaves the next start alone.
