---
id: protocol-audit-client-xfer-download-bounds
title: The client's Xfer download has no size cap and no ordering check
topic: protocol
status: done
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

## Fixed (2026-08-28)

Both gaps, on `Session`'s `SendXferPacket` handler, and the fix turned out to
be closer to the reference than the simulator's own.

**Ordering.** `XferDownload` carries the packet number the stream is waiting
for. `LLXferManager::processReceiveData` makes two distinctions and both are
now made here: a repeat of the packet just taken is the sender not having seen
our `ConfirmXferPacket`, so it is confirmed *again* and its bytes dropped;
anything else is a gap, logged and ignored, with the transfer left live so the
packet actually awaited can still arrive (and `XFER_STALL_TIMEOUT` catching a
stream that never recovers). Neither case appends, which is what used to
corrupt the assembled file.

**Size.** The primary bound is the sender's own: packet 0's four-byte prefix
declares the total, and the stream may not deliver more. This is what the
reference enforces — `LLXfer_Mem::setXferSize` allocates exactly that many
bytes and `LLXfer::receiveData` will not append past the allocation — and both
grids write the true total, OpenSim included
(`LLClientView.SendXferPacket` writes `XferData.Length`). A declared length of
**zero** is read as "declared nothing" rather than as an empty file: a real
empty file carries no payload and so never reaches the bound anyway, while a
sender that simply left the prefix unfilled would otherwise have its whole
stream refused. Such a stream — and any stream whose declaration is larger than
a real file — stops at `MAX_XFER_DOWNLOAD_BYTES` (16 MiB, sized as on the
simulator side: a region's terrain RAW is 832 KiB).

A refused download is aborted rather than completed from a wrong buffer:
`abandon_xfer` now takes the `LLTErrorCode` to report, so the refusal sends
`AbortXfer` with the reference's `LL_ERR_CANNOT_OPEN_FILE` (`-42`, what
`processReceiveData` aborts with when the receiver cannot take the bytes)
instead of the stall path's `LL_ERR_TCP_TIMEOUT`, and surfaces the same
`Diagnostic::ExpectedReplyMissing` + `Event::XferAborted` pair so a caller
waiting on the file is not left hanging.

Four tests in `tests/lifecycle.rs`: a gap is ignored and leaves the assembled
file unpolluted, a repeat is re-confirmed without doubling the bytes, a stream
past its declared length is refused with `-42` and dropped from the registry,
and an undeclared stream stops at the ceiling on exactly the packet that would
cross it. All of them frame their packets through `sl-wire`'s own
`encode_xfer_chunk` rather than hand-rolling the prefix, and the two existing
round-trip fixtures (mute list, terrain RAW) were moved onto it too — so they
declare their real length instead of a zero prefix and exercise the
declared-length path the way a real simulator drives it.
