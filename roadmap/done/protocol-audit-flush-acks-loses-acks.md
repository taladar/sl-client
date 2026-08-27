---
id: protocol-audit-flush-acks-loses-acks
title: flush_acks drops every remaining ack on a wire error, in both directions
topic: protocol
status: done
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

## Fixed (2026-08-27)

The batching rule and the failure policy move into one new private module,
`sl-proto/src/ack_flush.rs`, which both directions now call: the duplicated
`MAX_ACKS_PER_PACKET` constants (`session.rs:48`, `sim_session.rs:305`) collapse
into it, and each `flush_acks` is down to taking `pending_acks` and handing the
batch to `send_ack_packets`.

`send_ack_packets` offers **every** chunk to the sender even after one fails,
and returns the first error once they all have. Re-queueing the failing chunk
was the other option and is worse: the only way the encode can fail is the
block-count `u8` overflowing (`VariableTooLong { max: 255 }`), which is a
function of the message's own contents, so it would fail identically on every
later flush and wedge every ack queued behind it forever. The chunking is what
makes that unreachable in the first place, so the policy only decides what an
impossible failure costs — and the answer is now "one chunk", not "the rest of
the batch".

Both callers were doing the wrong thing with the error, for opposite reasons.
The server's `handle_timeout` swallowed it outright with a comment about
dropping the owed acks; it now logs it (`handle_timeout` returns `()` and must
not fail the session over an encode bug), and the comment says what is actually
true. The client's child-circuit loop propagated it with `?` from *inside* the
loop, which skipped every remaining child **and** the dead-child sweep after it;
a child never fails the session, so it logs and carries on. The root circuit
keeps propagating.

Five tests on the policy itself, driving `send_ack_packets` with a closure
instead of a circuit: chunking at the bound in order, nothing dropped, and the
two failure cases — a mid-batch failure still offers all four chunks (two, under
the old `?`), and the *first* error is the one reported. The sixth pins the
tests' `u32` chunk constant to the module's `usize` one.
