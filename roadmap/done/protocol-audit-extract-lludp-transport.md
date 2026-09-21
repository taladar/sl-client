---
id: protocol-audit-extract-lludp-transport
title: The LLUDP reliable-transport layer is implemented twice and has drifted
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 8
refs: [protocol-audit-flush-acks-loses-acks, protocol-audit-dispatch-child-drift]
---

Context: [context/protocol.md](../context/protocol.md).

The client circuit and the server session carried byte-identical copies of the
reliable-transport layer:

- `SeenWindow` + `insert` — `session.rs:731-752` was byte-identical to
  `sim_session.rs:345-370`;
- `UnackedPacket` — `session.rs:713` vs `sim_session.rs:336`;
- `queue_ack` / `record_acks` / `flush_acks` / `process_resends` /
  `next_resend_deadline` / `note_received` — `circuit.rs:5905-6000` vs
  `sim_session.rs:7405-7475`;
- the seven transport constants — `session.rs:32-48` vs
  `sim_session.rs:278-305`.

Nothing kept them in sync, and they **had** already drifted: the client's
`process_resends` removed an exhausted packet and reported `(sequence, name)`;
the server's set a bool, left the entry in `unacked`, and dropped the `name`
field — so a sim-side give-up was anonymous. (That one had since been repaired
by hand, in both places, which is the shape of the problem.)

**Done** (2026-09-20). `sl-proto/src/link.rs` is the layer, once: a
`ReliableLink` owning the sequence counter, the unacked map and its resend
policy, the owed-ack batch, the seen window, the outbound queue, the two
transport deadlines (inactivity, ack flush) and the keep-alive ping whose
round-trip average the resend timeout is derived from — plus the eleven
constants and the `deadline` / `merge_deadline` helpers that were also copies.
`Circuit` and `SimSession` each hold one and keep only what is genuinely
theirs: `severity_of`, which of *their* messages the session cannot survive
losing. Both sides' public method surfaces are unchanged (they delegate), so no
caller moved.

Three copies collapsed to one in passing: `SimReliableSeverity` became
`ReliableSeverity`, and the simulator's first ping id moved from 1 to 0 (the
client's, and the reference's `mLastPingID`); ping ids are opaque echoes, so
nothing reads either value. One behaviour is deliberately different:
`SimSession::start_ping_check` now answers `None` on a closed session rather
than minting an id for a datagram `send` would refuse to queue.

What this does **not** touch, and why: the four fixes named as follow-ons here
were already applied to both copies by hand before this landed
([[protocol-audit-flush-acks-loses-acks]] among them), so the extraction
removes the hazard rather than a live bug. The next one to gain from it is
[[protocol-audit-session-god-object]] — the transport half is now out of both
god objects.
