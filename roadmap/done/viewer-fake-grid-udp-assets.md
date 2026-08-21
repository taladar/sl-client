---
id: viewer-fake-grid-udp-assets
title: Fake grid answers the legacy UDP asset paths — Xfer, Transfer, task inventory
topic: viewer
status: done
origin: protocol-sim-http-misc audit (2026-08-21) — fake-grid coverage gap
points: 3
refs: [viewer-fake-grid, protocol-sim-http-misc, protocol-sim-udp-flows]
blocked_by: [viewer-fake-grid]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-fake-grid` serves assets exclusively over CAPS (`GetTexture`,
`GetMesh`, `ViewerAsset` from `Scenario.assets`). Its driver acts only on
`ServerEvent::AgentArrived`; `XferRequested`, `TransferRequested`, and
`RequestTaskInventory` are merely rebroadcast, so a client calling
`fetch_task_item_asset`, `fetch_estate_covenant_asset`,
`fetch_task_inventory`, or `request_xfer` against the fake grid hangs
until its own timeout — even though `SimSession` implements every one of
these flows (`register_xfer_file`, `serve_task_inventory`,
`send_transfer_asset` / `send_transfer_fail`). The upload direction
already works because the runtime sets the secure session id and
`SimSession` handles `AssetUploadRequest` end to end.

Add to `Scenario`: named Xfer files, per-task inventories (for
`serve_task_inventory`), and a transfer-source resolver (task item / estate
covenant → bytes); teach the driver to answer the three events from those
fixtures (and `send_transfer_fail` for unknown sources). Cover with
`client_end_to_end.rs` cases driving the real `sl-client-tokio` methods.

Update (2026-08-21): the server half of the terrain RAW flows now exists
too ([[protocol-sim-terrain-raw-flows]] — `send_initiate_download`,
`request_xfer_upload`, `TerrainDownloadRequested` / `TerrainUploadRequested`
/ `XferReceived`), so a terrain fixture (a RAW heightmap answered on
`TerrainDownloadRequested`, an uploaded one captured from `XferReceived`)
belongs in the same `Scenario` growth.

**Done (2026-08-21).** `sl-fake-grid` gained `udp_assets::UdpAssetFixtures`
on `Scenario` (named `Xfer` files, task inventories by local id, task-item
asset bodies, the estate covenant, the terrain RAW heightmap) plus a
generic `Scenario::on_event` hook. The driver's flush answers
`RequestTaskInventory` (`serve_task_inventory`), `TransferRequested`
(`send_transfer_asset` / `send_transfer_fail(UnknownSource)`),
`TerrainDownloadRequested` (`send_initiate_download`),
`TerrainUploadRequested` (`request_xfer_upload`), keeps an `XferReceived`
upload as the new heightmap, and re-arms a served named `Xfer` file (a
`SimSession` registration is consumed per request). The stock scenario
ships `motd.txt`, a scripted object with a script body, a covenant, and a
flat 25 m RAW32 heightmap (`flat_terrain_raw`). Covered by four new
`client_end_to_end.rs` cases driving the real `sl-client-tokio` commands
(Xfer download + re-arm + unknown-name abort, task inventory → item asset
→ unknown-item refusal, covenant, terrain download → upload → re-download).
Bake stays event-only (no revert baseline). Book chapter updated.
