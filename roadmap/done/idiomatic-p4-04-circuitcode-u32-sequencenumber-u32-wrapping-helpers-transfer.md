---
id: idiomatic-p4-04
title: CircuitCode(u32), SequenceNumber(u32) (wrapping helpers), TransferId(U
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 4 — Domain ID newtypes (medium-high invasiveness)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`CircuitCode(u32)`, `SequenceNumber(u32)` (wrapping helpers),
`TransferId(Uuid)`/`XferId(u64)`, `PingId(u8)`, `InventoryCallbackId(u32)`.
Six bookkeeping/correlation id newtypes. Two are wire concepts and live in
`sl-wire`: **`CircuitCode(pub u32)`** (`sl-wire/src/circuit_code.rs`, the
login server's per-session code reused by every circuit — explicitly *not* the
same as the local per-connection `CircuitId`; a separate type) and
**`SequenceNumber(pub u32)`** (`sl-wire/src/sequence_number.rs`, `FIRST` +
`wrapping_next` helper). `SequenceNumber` was taken to **full depth** (the
user-approved maximal option): `ParsedDatagram.sequence`, `.acks`, and the
`encode_datagram`/`parse_datagram` framing primitives are typed, as is all
the session-layer ack bookkeeping (`next_sequence`, `pending_acks`, the
`unacked` `BTreeMap` key, the `SeenWindow` set/queue) in both `Session` *and*
`SimSession`, plus the public `Diagnostic::ExpectedReplyMissing.sequence`. The
other four are session correlation ids in
**`sl-proto/src/bookkeeping_ids.rs`** (all public, re-exported):
**`PingId(pub u8)`** (`wrapping_next`; `ServerEvent::PingRequested`,
`SimSession::start_ping_check`), **`XferId(pub u64)`** (the legacy
file-transfer id; `mute_xfers`/`upload_xfers` keys, the
`send_*_xfer_*`/`advance_upload` params), **`TransferId(pub Uuid)`** —
wrapping the **actual wire `LLUUID`** (the `u128` `next_transfer_id` is only
the minting counter, so the roadmap's literal `(u128)` was wire-incorrect;
user-approved `TransferId(Uuid)` with a `from_u128` minting helper keys the
`asset_transfers` map) — and **`InventoryCallbackId(pub u32)`**
(`Event::InventoryItemCreated.callback_id`, the `InventoryBulkUpdate`
`(item_id, callback_id)` pairs, and the `create`/`copy_inventory_item` return
values). The public `Session::circuit_code` accessor now returns
`Option<CircuitCode>`. Codec wraps/unwraps `.0`/`.get()` at every boundary so
the wire bytes are byte-identical. Re-exported through
`sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity, including the runtime
`circuit_code()` accessor + bevy `SlIdentity.circuit_code`); `sl-repl`
`SessionContext` keeps a typed `circuit_code` and the `set_identity` arg.
+3 unit tests in `circuit_code`/`sequence_number`, +4 in `bookkeeping_ids`
(round-trips, wrapping, the `from_u128` mint); lifecycle + `sim_session`
round-trip suites updated. The wire-spec prose in `book/` is unchanged (these
are representation-only wrappers — a circuit code / sequence number is still
the same concept). NO sl-types touched (all client wire/session concepts).
**Phase 4 COMPLETE — next effort = Phase 5 typed UUID keys from `sl-types`.**
