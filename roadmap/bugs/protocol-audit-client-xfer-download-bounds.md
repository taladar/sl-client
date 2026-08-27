---
id: protocol-audit-client-xfer-download-bounds
title: The client's Xfer download has no size cap and no ordering check
topic: protocol
status: bugs
origin: found while fixing protocol-audit-sim-session-lifecycle (2026-08-28)
points: 2
refs: [protocol-audit-sim-session-lifecycle]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:3647` — the client's `SendXferPacket`
handler is the mirror image of the simulator hole fixed in
[[protocol-audit-sim-session-lifecycle]], and has the same two gaps:

- `download.buffer.extend_from_slice(chunk.payload)` grows with **no size
  cap**. A simulator (or anything that can spoof one onto the circuit) that
  never sets the end-of-file flag grows the buffer for as long as it keeps
  sending.
- there is **no ordering check**. `Xfer` is a strictly ordered,
  one-packet-in-flight stream, but any packet number is concatenated, so a
  duplicate or a re-ordered packet silently corrupts the assembled file rather
  than being refused.

The simulator side already has both (`MAX_XFER_RECEIVE_BYTES` and a
`next_packet` expectation, refusing with `RejectionReason::OutOfOrder` /
`LimitExceeded`); this is the same pair on `Session`, reported through a
`Diagnostic` and an `AbortXfer` rather than a `ServerEvent`.

The client does at least reap a stalled download (`XFER_STALL_TIMEOUT`), so
this is bounded in *time* but not in *bytes*.
