---
id: protocol-sim-udp-flows
title: Server-side state machines for the higher-level LLUDP flows
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 8
refs: [protocol-sim-caps-inventory, viewer-fake-grid]
---

Context: [context/protocol.md](../context/protocol.md).

Client `Session` implements flow-level support *above* individual messages
for several multi-step protocols; `SimSession` has the message-level
encode/decode but not the mirroring flows. General principle (write it
into context too): **for every high-level flow the client `Session`
implements, `SimSession` gains the mirroring server-side state machine**,
verified by in-memory `Session` ↔ `SimSession` loopback tests extending
`sl-proto/tests/sim_session.rs`.

Concretely in this task:

- the UDP transaction asset upload (AssetUploadRequest → Xfer receive →
  assembled asset → AssetUploadComplete — the path the wearable in-place
  save uses);
- Xfer file serving in the send direction (e.g. task-inventory files);
- the legacy TransferRequest → TransferInfo/TransferPacket asset download
  streaming;
- FetchInventoryDescendents → InventoryDescendents serving over UDP from
  the shared inventory fixture (see [[protocol-sim-caps-inventory]]);
- the teleport flow (TeleportRequest → TeleportStart/Progress/Finish
  incl. child-agent circuit establishment /
  EstablishAgentCommunication).

Opens with an audit item: enumerate the flow-level machines in client
`Session`/`session/` submodules and pin the mirror coverage as a committed
table (same convention as the CAPS coverage table), filing follow-up tasks
for flows beyond this task's list (money, friendship, group sessions,
appearance, …) rather than growing unbounded.

Done (2026-08-13): the flow mirrors landed with two scope changes under
the new legacy-skip rule (a legacy UDP flow is skipped when BOTH grids
offer a modern alternative — now written into
[context/protocol.md](../context/protocol.md)):

- **UDP `FetchInventoryDescendents` serving was re-scoped to a pinned
  `Legacy` skip** — both grids serve the `FetchInventoryDescendents2`
  cap (Firestorm's UDP send is unreachable when it exists), so the
  CAPS serving side and the inventory-tree fixture now belong entirely
  to [[protocol-sim-caps-inventory]].
- **The Transfer download was net-new on BOTH sides** (the client had
  no `TransferRequest` flow at all; it fetches plain assets via
  `ViewerAsset`): only the two still-live sources are implemented —
  `SimInvItem` (task-item assets, no cap on either grid) and
  `SimEstate` (covenant); the plain-asset source is refused as legacy.

What landed: SimSession Xfer file serving + `serve_task_inventory`
(with the `build_task_inventory` listing writer mirroring the parser),
the transaction asset-upload receive (inline + Xfer pull by predicted
`VFileID`), the `sl-wire/src/transfer.rs` params codecs + client
`fetch_task_item_asset`/`fetch_estate_covenant_asset` + SimSession
`send_transfer_asset`/`send_transfer_fail`, teleport serving
(request/lure events, `send_teleport_*` mechanics, the event-queue
trio wrappers, `AgentPresence` child/root tracking,
`send_disable_simulator`), and the pinned flow-coverage table
(`SESSION_FLOW_COVERAGE` + `flow_coverage_table_is_pinned`) with
`Pending` rows split out to [[protocol-sim-udp-flows-2]]. Loopback
tests extend `sl-proto/tests/sim_session.rs` (incl. the two-SimSession
inter-region teleport via `pump_multi`); book coverage: the new
`book/src/comms/transfer.md` chapter plus "The server side" sections
in `comms/xfer.md` and `content/teleport.md`. SL's inline-LLSD
`RequestTaskInventory` cap (no OpenSim support) is noted for the CAPS
cluster tasks. Follow-up live verification: exercise the new client
Transfer fetch against OpenSim (read a script body out of a prim).
