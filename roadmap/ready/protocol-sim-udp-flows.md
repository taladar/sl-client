---
id: protocol-sim-udp-flows
title: Server-side state machines for the higher-level LLUDP flows
topic: protocol
status: ready
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
