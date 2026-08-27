---
id: protocol-audit-texture-download-completeness
title: A texture download completes on a chunk count, not on the chunk indices
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session.rs:792` — `TextureDownload::is_complete` was
`usize::from(self.packets) == self.chunks.len()`, where `packets` comes off the
wire and `chunks.insert(packet_index, ...)` accepted any `u16`.
`TransferDownload::is_complete` counted the same way (`chunks.len() ==
last_packet + 1`), and neither enforced the size its header declared.

Out-of-range or duplicated packet indices therefore "completed" a download that
was missing real packets, and `assemble()` concatenated across the gap —
producing a corrupt codestream, or a notecard/script body that parses and is
wrong, with nothing to distinguish it from the real asset.

Both now complete on the **index set**, which is what the reference does from
the other side — it delivers packets strictly in order and acts on the
terminating status only when that packet's turn comes, so its
`mLastPacket >= mTotalPackets - 1` is exactly a contiguous run from zero.

Fixed here, all on that one rule:

- **Textures.** `is_complete` is a contiguous walk from packet 0, and a header
  that has not arrived yet (`packets == 0`) is never complete however many
  follow-on packets are buffered. `insert_chunk` drops an index at or past the
  header's count and keeps the first copy of a repeated index (the reference's
  `LLTextureFetchWorker::insertPacket`), and `note_header` prunes what a packet
  that outran the header left behind that the header now disowns.
- **Transfers.** The same contiguous walk, plus `note_last_packet`, which drops
  anything buffered past the `Done` index — the reference stops delivering
  there and discards the rest of its delayed map, so a higher index is not part
  of the asset and must not be concatenated onto the end of it.
- **The declared size is enforced.** `expected_size` was recorded and never
  read; the assembled length is now held to it, and a stream that overruns it or
  says `Done` short of it fails the fetch (`Event::TransferFailed` with
  `TransferStatus::Error`) with a `TransferAbort` on the wire, rather than
  handing a caller a truncated asset.
- **A negative packet index is not packet zero.** The wire field is signed and
  was `u32::try_from(...).unwrap_or(0)`, so a negative index overwrote the real
  first packet with a stranger's bytes. It is dropped now.
- **The out-of-order buffer is bounded** by the reference's
  `LL_MAX_DELAYED_PACKETS` (100 packets past the contiguous prefix), so a
  simulator streaming nothing but far-future indices — none of it ever
  deliverable — is abandoned instead of buffered without limit.

Six tests in `lifecycle.rs`, each of which the old rule fails: a texture whose
stray and repeated indices make the count add up while packet 2 is missing, a
packet that outran its header and the header then disowns, a transfer with a
`Done` at index 2 over a missing packet 1, a transfer that ends short of its
declared size, one drowned in out-of-order packets, and a negative index that
must not land on packet 0.
