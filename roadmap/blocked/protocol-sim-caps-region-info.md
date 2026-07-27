---
id: protocol-sim-caps-region-info
title: Server-side region/object-info caps
topic: protocol
status: blocked
origin: user request (2026-07) — complete simulator protocol surface
points: 5
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The region- and object-information cluster, server side:

- `SimulatorFeatures` (paired with `sl-wire/src/sim_features.rs`);
- `LSLSyntax` (paired with `sl-wire/src/lsl_syntax.rs`);
- `ExtEnvironment` — EEP get/put;
- `RemoteParcelRequest` (paired with `sl-wire/src/remote_parcel.rs`);
- `GetObjectCost` / `GetObjectPhysicsData` / `ResourceCostSelected`
  (paired with `object_cost.rs` / `object_physics.rs` /
  `resource_report.rs`);
- `AttachmentResources` / `LandResources`.

Inverse-pairing per the convention; verified against the client-direction
builders/parsers in-memory.
