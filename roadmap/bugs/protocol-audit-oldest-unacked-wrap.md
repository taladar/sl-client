---
id: protocol-audit-oldest-unacked-wrap
title: OldestUnacked uses numeric min instead of wrapping-oldest
topic: protocol
status: bugs
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
