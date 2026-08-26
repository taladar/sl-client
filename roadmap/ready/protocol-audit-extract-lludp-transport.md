---
id: protocol-audit-extract-lludp-transport
title: The LLUDP reliable-transport layer is implemented twice and has drifted
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [protocol-audit-flush-acks-loses-acks, protocol-audit-dispatch-child-drift]
---

Context: [context/protocol.md](../context/protocol.md).

The client circuit and the server session carry byte-identical copies of the
reliable-transport layer:

- `SeenWindow` + `insert` — `session.rs:731-752` is byte-identical to
  `sim_session.rs:345-370`;
- `UnackedPacket` — `session.rs:713` vs `sim_session.rs:336`;
- `queue_ack` / `record_acks` / `flush_acks` / `process_resends` /
  `next_resend_deadline` / `note_received` — `circuit.rs:5905-6000` vs
  `sim_session.rs:7405-7475`;
- the seven transport constants — `session.rs:32-48` vs
  `sim_session.rs:278-305`.

Nothing keeps them in sync, and they **have** already drifted: the client's
`process_resends` (`circuit.rs:5987`) removes an exhausted packet and reports
`(sequence, name)`; the server's (`:7452`) sets a bool, leaves the entry in
`unacked`, and dropped the `name` field — so a sim-side give-up is anonymous.

Scope: extract one `Circuit` / `Link` type owning the seen window, the unacked
map, the ack queue and the resend policy, and have both sessions hold it. This
also fixes [[protocol-audit-flush-acks-loses-acks]] once instead of twice, and
removes the constant-duplication hazard.
