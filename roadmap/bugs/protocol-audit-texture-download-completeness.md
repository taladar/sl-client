---
id: protocol-audit-texture-download-completeness
title: A texture download completes on a chunk count, not on the chunk indices
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session.rs:792` — `TextureDownload::is_complete` is
`usize::from(self.packets) == self.chunks.len()`, where `packets` comes off the
wire (`methods.rs:3785`) and `chunks.insert(packet_index, ...)` accepts any
`u16` (`:3800`).

Out-of-range or duplicated packet indices therefore "complete" a download that
is missing real packets, and `assemble()` (`session.rs:798`) silently
concatenates across the gap — producing a corrupt codestream that looks
complete. `TransferDownload` records `expected_size` (`:841`) and never enforces
it either.

Fix: verify the index set is exactly `0..packets`, and check the assembled
length against `expected_size` where one is known.
