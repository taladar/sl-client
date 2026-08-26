---
id: protocol-audit-asset-transfer-timeouts
title: No asset transfer has a timeout — six registries only ever grow
topic: protocol
status: bugs
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
