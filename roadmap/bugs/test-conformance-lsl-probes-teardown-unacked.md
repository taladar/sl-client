---
id: test-conformance-lsl-probes-teardown-unacked
title: The last derez and both logouts of an OpenSim probe run go unacknowledged
topic: test
status: bugs
origin: gridspec-lsl-runtime-errors (2026-10-27)
refs: [gridspec-lsl-runtime-errors, test-conformance-cancelled-case-leaves-its-prims]
---

Context: [context/test.md](../context/test.md).

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
