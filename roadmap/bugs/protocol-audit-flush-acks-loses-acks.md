---
id: protocol-audit-flush-acks-loses-acks
title: flush_acks drops every remaining ack on a wire error, in both directions
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/circuit.rs:5941` does
`let acks = std::mem::take(&mut self.pending_acks);` and then sends each chunk
with `self.send(...)?`. A failure on chunk *n* discards chunks *n..*, which are
no longer in `pending_acks` and will never be re-sent — so the peer retransmits
packets we already have.

The server copy at `sl-proto/src/sim_session.rs:7426` has the identical bug, and
there the caller swallows the error outright
(`sim_session.rs:9082: let _result = self.flush_acks(now);`).

Fix: take per chunk, or restore the un-sent remainder on failure. Neither path
has a test — the transport core in `circuit.rs` has no inline tests at all, so
this is only reachable through `tests/lifecycle.rs`, where an ack loss is
invisible.
