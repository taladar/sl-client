---
id: test-conformance-lsl-probes-teardown-unacked
title: The last derez and both logouts of an OpenSim probe run go unacknowledged
topic: test
status: done
origin: gridspec-lsl-runtime-errors (2026-10-27)
refs: [gridspec-lsl-runtime-errors, test-conformance-cancelled-case-leaves-its-prims]
---

Context: [context/test.md](../context/test.md).

Resolved (2026-11-04): the grid's, written up in
`book/src/gridspec/session.md` § Logout. Neither suspect held. The client
reads its socket throughout, and the runner waits for the derez's
`KillObject` before it logs out.

Reproduced with five probes in one session (single-probe runs never showed
it), with a temporary trace of every acknowledgement the client took in.
The last one before the teardown covered an earlier packet. The derez's
`KillObject` and its Trash item (`UpdateCreateInventoryItem`) arrived
carrying none. The `LogoutRequest` followed 2 ms later. OpenSim's close
clears every acknowledgement it still owes along with the reply, so a derez
whose acknowledgement had not yet gone out on OpenSim's half-second flush
is carried out and never acknowledged.

Both logouts going unanswered is the race already measured in
[[protocol-logout-reply-sometimes-missing-on-opensim]]. In these runs the
second avatar's reply did come, within 2 ms of its request; it was logged
just after the first avatar's five-second timeout, which made it look late.
The fake OpenSim already drops owed acknowledgements on close.

Nothing to change in the harness. Waiting half a second before a logout
would save the derez's acknowledgement, but the logout's own warnings
would stay.

## Observation

Every `sl-conformance run --grid opensim … lsl-probes` (and
`lsl-probes-xengine`) of 2026-10-27 ended the same way, whichever probes
ran (`WARN sl_proto::session::methods:` lines):

```text
reliable packet exhausted its retransmission budget sequence=1086 DeRezObject
reliable packet exhausted its retransmission budget sequence=1087 LogoutRequest
logout timed out waiting for LogoutReply
reliable packet exhausted its retransmission budget sequence=563 LogoutRequest
logout timed out waiting for LogoutReply
```

The case still passes. OpenSim's log has both logouts arriving ("Got a
logout request for …", the agent closed in all four regions) at the second
the client sent them, and the region DB holds none of the last probe's
prims afterwards, so the derez and the logouts arrived; it is their **acks
and the `LogoutReply`** that the client never took in. The aditi runs of
the same day ended cleanly.

## Suspects

- the case's last `wait_for` returning and nothing pumping the sessions'
  inbound queue while the derez is in flight, so the acks sit unread until
  the retransmission budget is spent;
- the logout sent before the derez's ack, and OpenSim closing the circuit
  without acking what it had.

## Done when

An OpenSim probe run ends with the derez acked and a `LogoutReply` for both
avatars, or the cause is shown to be the grid's and noted here.
