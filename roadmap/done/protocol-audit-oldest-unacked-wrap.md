---
id: protocol-audit-oldest-unacked-wrap
title: OldestUnacked uses numeric min instead of wrapping-oldest
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/circuit.rs:500` picks the oldest unacked packet with
`self.unacked.keys().next()` over a `BTreeMap<SequenceNumber, _>` using derived
numeric `Ord`, while `next_sequence` advances with `wrapping_next`
(`circuit.rs:375`).

After a `u32` sequence wrap the numerically smallest key is the **newest**
packet, so the `OldestUnacked` field reported to the simulator is wrong.

Practically unreachable at viewer packet rates (about 1.4 years at 100 pkt/s),
so this is filed for correctness rather than urgency — but the invariant is
unguarded and a test over a synthetic wrapped window is trivial.

## Resolution

**The counter is the seam of the circle.** The unacked set is not an interval
of the integers but of the wrapping `u32` counter, and the one value that
separates its two halves is `next_sequence` itself: everything outstanding was
sent before it, so a key *above* the counter is one the counter has already
passed and wrapped away from — older than every key below it, which was sent
since the wrap. The oldest entry is therefore the first key strictly greater
than the counter, and only when there is none (the un-wrapped case, and the
only one a viewer normally reaches) the numerically smallest. That is the same
split the reference viewer makes with
`mUnackedPackets.upper_bound(getPacketOutID())` in
`LLCircuitData::pingTimerExpired` (`llcircuit.cpp:891`), under a comment saying
exactly why.

**Nothing outstanding reports the counter, not zero.** Both call sites reported
`0` for an empty set, which is not a conservative default — `OldestUnacked` is
what lets the peer retire its duplicate-suppression record
(`LLCircuitData::clearDuplicateList`), and `0` retires none of it, so a quiet
circuit pinned the peer's record forever. The reference reports
`getPacketOutID()`, one past every sequence number the peer could still be
holding a record for, which retires all of it.

**One rule, both directions.** The client circuit and the simulator session
keep the same map and had the same two defects (`sim_session.rs:7519` was a
verbatim copy), so the rule lives in `sl-proto/src/unacked.rs` — a sibling of
`ack_flush.rs`, which already houses the ack-batching rule both directions
share — with six tests over synthetic wrapped windows: the un-wrapped set, a
wrap in the middle of the range, a contiguous run straddling `u32::MAX`, a set
lying entirely above the counter, a single packet on either side, and the empty
set.

The reference's *other* known wrap divergence is left alone deliberately:
`process_resends` retransmits in the map's numeric order, so after a wrap it
resends out of order. `clearDuplicateList`'s comment (`llcircuit.cpp:284-289`)
records that the reference does the same, on the grounds that resends are
already out of order.
