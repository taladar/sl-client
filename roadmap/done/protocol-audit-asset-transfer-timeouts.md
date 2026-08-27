---
id: protocol-audit-asset-transfer-timeouts
title: No asset transfer has a timeout — six registries only ever grow
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-inventory-fetch-deadlock]
---

Context: [context/protocol.md](../context/protocol.md).

`struct Timers` (`sl-proto/src/session.rs:756`) has fields for inactivity, ack
flush, agent update, ping, logout, teleport and sit — and **nothing for any
asset transfer**. So `texture_downloads` (`methods.rs:8211`),
`transfer_downloads` (`:8024`), `xfer_downloads` (`:7915`), `xfer_uploads`,
`pending_xfer_uploads` (`:10983`) and `pending_asset_uploads` (`:10132`) are
insert-on-request, remove-on-success only. A stalled stream strands its partial
`chunks` map — and for uploads, whole asset payloads — for the session's life.

Two more entries on the same list:

- `requested_parents` (`methods.rs:2419`) dedupes so an unknown parent is
  requested **exactly once, ever**, and the request result is discarded
  (`:2580`). If that one speculative fetch is dropped the linkset child never
  resolves, and `forget_sim_objects` (`:2747`) does not clear the set, so
  retired circuits' ids accumulate.
- `pending_task_inventory_unresolved: VecDeque<()>` (`session.rs:1388`) is a
  unit-typed deque used as a counter. A fetch whose reply never arrives leaves
  an entry behind that later **hijacks an unrelated `ReplyTaskInventory`**.

Related, and worth folding in: `start_xfer_download` (`methods.rs:7915`) returns
`Ok(xfer_id)` having sent nothing when there is no circuit — the registry entry
is inserted first, so the caller waits forever on a request that never went out.
`start_transfer_download` (`:8024`) has the same shape, and `request_texture`
(`:8211`) inserts before its `NoCircuit` bail at `:8219`.

## Fixed (2026-08-27)

Every one of the eight registries now carries the instant it last made
progress, on the entry itself rather than in a side map, and one sweep —
`Session::expire_asset_transfers`, run from `handle_timeout` beside the
inventory prune — advances them all. The earliest armed deadline is merged into
`poll_timeout`, so an otherwise idle shell still wakes for a stream that went
quiet.

Each timeout is the reference viewer's, so the client gives up when the
reference gives up:

| Registry | Timeout | Reference |
| --- | --- | --- |
| `texture_downloads` | 10 s, then re-request; 10 retries | `SIM_LAZY_FLUSH_TIMEOUT`, `LL_PACKET_RETRY_LIMIT` |
| `transfer_downloads` | 5 min | `LL_ASSET_STORAGE_TIMEOUT` |
| `xfer_downloads` / `xfer_uploads` | 30 s | `LL_PACKET_TIMEOUT` x `LL_PACKET_RETRY_LIMIT` |
| `pending_xfer_uploads` / `pending_asset_uploads` | 60 s | `LL_XFER_REGISTRATION_TIMEOUT` |

Textures **retry rather than fail**, which is what the reference does: a stall
re-issues the `RequestImage` resuming at the first packet index still missing
(`mLastPacket + 1`), and an arriving packet restores the full retry budget. The
reference's texture path has no retry ceiling of its own because a worker there
is dropped when the viewer stops wanting the image; a sans-I/O session has no
such signal, so an unbounded retry would be an unbounded registry — hence the
borrowed `LL_PACKET_RETRY_LIMIT`.

Every give-up surfaces on an **existing** event, so no consumer has to learn a
new variant to stop hanging: `Event::TextureNotFound` (which already meant "this
image is not coming", including on a failed HTTP fetch), `Event::TransferFailed`
with `TransferStatus::Abort`, `Event::XferAborted` — and a new client-side
`AbortXfer` / `TransferAbort` goes out so the simulator stops serving a stream
nobody is reading. `XferAborted` carries `LL_ERR_TCP_TIMEOUT` (`-23016`), the
code `LLXfer::abort` itself sends on a timeout. An expired *asset* upload offer
additionally fails its save with a `success: false`
`Event::InventoryAssetSaved`, since `AssetUploadComplete` is that path's only
other completion. Each one also
pushes a `Diagnostic::ExpectedReplyMissing` under a new per-path request label.

The two non-transfer entries needed different shapes:

- `requested_parents` became a **cooldown** keyed map rather than a latch.
  Asking once ever strands every child of a root the simulator ignored; asking
  on every referencing update floods a region streaming many children of one
  missing root.
  So the dedupe expires after `RELIABLE_REPLY_GRACE` (60 s — deliberately longer
  than the reliable layer's own worst-case retransmission window of
  `RELIABLE_TIMEOUT_FACTOR` x `PING_AVERAGE_MAX` x `MAX_RESEND_ATTEMPTS` = 40 s,
  so a re-ask never races the original still being retransmitted), and
  `forget_sim_objects` now drops the retiring circuit's entries — it was the one
  per-circuit store that outlived its circuit.
- `pending_task_inventory_unresolved` holds instants instead of units, and the
  same grace drops a claim whose reply never came. `pending_task_inventory` (the
  resolved half) got the same treatment: it cannot hijack a *different* object's
  reply, but a stale entry there silently upgrades a later plain
  `request_task_inventory` for the same object into a full fetch-and-parse.

The three starters that registered before sending — `start_xfer_download`,
`start_transfer_download`, `request_texture` — now send first and register only
on success, so neither a missing circuit nor a failed encode leaves an entry a
caller would wait on forever.

The four in-flight structs dropped their `serde` derives to make room for
`Instant` (which is not `Serialize`). They are private, and `Session` itself is
`Debug`-only, so nothing could ever have serialised them.

Seven tests in `sl-proto/tests/lifecycle.rs`: a texture download re-requested
from packet 1 on every stall and then abandoned (with its partial buffer gone —
late packets no longer assemble), a stalled Xfer download aborted on the wire
with `LL_ERR_TCP_TIMEOUT` and surfaced, a stalled asset transfer aborted and
failed, a terrain upload offer withdrawn (a late `RequestXfer` for it is no
longer honoured), an asset upload offer failing its save, a lost task-inventory
claim not hijacking an unrelated later reply, and an unanswered unknown-parent
request re-asked after the cooldown. The stall tests need minutes of wall clock,
so two helpers keep the circuit fed: one acknowledges the bootstrap packets so
`UseCircuitCode`/`CompleteAgentMovement` do not exhaust their retransmissions,
the other feeds an inert datagram every 30 s so the 45-second inactivity timeout
does not close the session out from under the test.
