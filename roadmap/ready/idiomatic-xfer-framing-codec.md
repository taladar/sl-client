---
id: idiomatic-xfer-framing-codec
title: Shared sl-wire Xfer framing codec + explicit Xfer/Transfer edge cases
topic: idiomatic
status: ready
origin: protocol-sim-http-misc audit (2026-08-21) — Xfer/Transfer coverage gaps
points: 2
refs: [protocol-sim-udp-flows, protocol-sim-http-misc]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

The [[protocol-sim-http-misc]] audit found the Xfer packet framing — the
sequence-0 little-endian `u32` size prefix and the `0x8000_0000`
end-of-file bit on the packet id — implemented four times by hand: client
receive and send (`sl-proto/src/session/methods.rs`, the `SendXferPacket`
handler and the upload sender) and server send and receive
(`sl-proto/src/sim_session.rs`, `send_next_xfer_send_packet` and the
`SendXferPacket` handler). The Transfer source params did get a shared
sans-I/O codec (`sl-wire/src/transfer.rs`); Xfer did not, and the chunk
size is a crate-private constant reached across modules
(`crate::session::XFER_UPLOAD_CHUNK_SIZE`).

- Add `sl-wire/src/xfer.rs`: `XferPacketId` (packet number + EOF bit),
  the first-packet size prefix encode/decode, `XFER_CHUNK_SIZE`, with
  byte-pinned tests; port all four sites onto it.
- `SimSession::abort_xfer` on an unknown `XferId` is silently a no-op and
  `send_next_xfer_send_packet` a documented no-op for a vanished send —
  give Xfer an `Error::UnknownXfer` analogue of `Error::UnknownTransfer`
  so a driver typo is observable.
- The `TransferRequest` responder refuses source type `Asset` (2) only by
  falling into the `else` with unknown/garbage sources; make the
  legacy-skip branch explicit (`TRANSFER_SOURCE_ASSET` match arm) and
  surface a `ServerEvent` so a driver can log that a client tried the
  legacy asset source.
