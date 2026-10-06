---
id: protocol-logout-reply-sometimes-missing-on-opensim
title: A logout on the local OpenSim sometimes gets no LogoutReply
topic: protocol
status: done
origin: OpenSim control runs while verifying viewer-sit-on-neighbour-object-uses-root-circuit (2026-09-17)
refs: [viewer-sit-on-neighbour-object-uses-root-circuit, gridspec-logout]
---

## Resolved (2026-10-06): OpenSim's race, not ours

Explained by [[gridspec-logout]] and written up in
`book/src/gridspec/session.md` § Logout. OpenSim queues the `LogoutReply`
and closes the agent in the same call; the close clears the outbox and the
pending acknowledgements, so the reply — and the `LogoutRequest`'s
acknowledgement with it — reaches the wire only when the outgoing-packet
thread wins the race. Measured: never within two seconds of arriving, one
logout in four after that. The request did reach the simulator every
time (its log shows the logout and the close), and the next login always got
in.

Nothing in the client was wrong: it resends, sometimes reports the request's
retransmissions exhausted just before its five-second logout timeout, and
ends the session as a logout. What did change is the viewer, whose
three-second quit deadline cut that timeout short; and the fake grid's
OpenSim flavour now withholds the reply so both are exercised.

## Observation

Headless `sl-repl-tokio --script` runs against the local OpenSim
(`--start 'uri:Default Region&250&134&25'`, a 20 s hold, then `logout`) end in
`logout timed out waiting for LogoutReply` in some runs and not in others. The
session still reports `logged_out`, because the 5 s `LOGOUT_TIMEOUT` gives up.

- Run 1 (a sit on a neighbour's object before the logout):
  `reliable packet exhausted its retransmission budget` for the
  `LogoutRequest`, then `ExpectedReplyMissing request=LogoutRequest`, then the
  logout timeout.
- Control run A (no sit): a clean `logged_out`.
- Control run B (no sit, started ~25 s after A logged out): the logout timeout
  again.

So it does not depend on the sit. Every resend of the `LogoutRequest` went
unacknowledged, as if OpenSim had already stopped answering the circuit.

## Investigate

- Capture a failing run with `tcpdump` + `sl-conformance-trace`: did the
  `LogoutRequest` reach the simulator at all, and did anything come back?
- Check the OpenSim log (`journalctl --user -u opensim.service`) at the
  logout time. Is the agent already being closed, perhaps from the previous
  run's logout (B started soon after A)?
- Check whether the root circuit's acks and pings were healthy just before the
  logout.
