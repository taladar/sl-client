---
id: protocol-41
title: Asset-transfer success event + declared size (extends #19, Tier C)
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**41. Asset-transfer success event + declared size (extends #19, Tier C). ✅
Done.** The `TransferInfo` handler (`session.rs`) emitted an event only on the
*failure* path; a successful transfer produced nothing, so `Size` (the total
asset byte size, useful for progress / preallocation) was lost. Added a new
**`Event::AssetTransferStarted { asset_id, asset_type, size }`** emitted on the
success path (status `LLTS_OK`/`LLTS_DONE`) — looking the in-flight transfer up
*without* removing it (the bytes still follow as `TransferPacket`s, reassembled
into the existing `Event::AssetReceived`); the failure path is unchanged
(`AssetTransferFailed`, which removes the transfer). `size` is surfaced as the
wire's `i32` (the simulator may send `0` when it does not know the size up
front). Re-exported through both runtimes via the shared `Event` type; the
`asset_fetch` tokio example logs the started event. Covered by the extended
`request_asset_reassembles_transfer_packets` `sl-proto` lifecycle test (asserts
the `AssetTransferStarted { sound, Sound, 6 }` fires on the `TransferInfo`
before the packets, then the `AssetReceived` reassembly). *Live-verified against
the local OpenSim via the `asset_fetch` example (`SL_ASSET_ID` = the default
sound `ed12…`): the UDP `TransferRequest` path surfaced `AssetTransferStarted …
(Sound, declared 9431 bytes)` immediately before `AssetReceived … (Sound, 9431
bytes)` — the declared size matching the reassembled asset exactly. Test: local
OpenSim.*
