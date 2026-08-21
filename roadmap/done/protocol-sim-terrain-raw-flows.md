---
id: protocol-sim-terrain-raw-flows
title: Server-side terrain RAW download/upload — InitiateDownload + Xfer pull
topic: protocol
status: done
origin: protocol-sim-http-misc audit (2026-08-21) — Xfer/Transfer coverage gaps
points: 3
refs: [protocol-sim-udp-flows, protocol-sim-http-misc, idiomatic-xfer-framing-codec]
---

Context: [context/protocol.md](../context/protocol.md).

The [[protocol-sim-http-misc]] audit of `SimSession`'s legacy UDP asset
coverage found the Xfer send/receive and `TransferRequest` responder
complete and pinned, with one flow family silently unhandled: the estate
terrain RAW transfer.

- `EstateOwnerMessage` is handled only for `method == "telehub"`
  (`sl-proto/src/sim_session.rs`, the `EstateOwnerMessage` arm); every
  other estate method — including `terrain` with `download filename` /
  `upload filename` — falls through with no `ServerEvent`, and the flow is
  not pinned as a `Legacy` skip in `SESSION_FLOW_COVERAGE` either.
- There is no server-side `InitiateDownload` sender: a driver can
  `register_xfer_file` but cannot *offer* a file, so the client's
  auto-follow (`Session` handles `InitiateDownload` → `RequestXfer`,
  `request_region_terrain_download`) is untestable against `SimSession`.
- There is no server-initiated Xfer *pull* for an arbitrary named file
  (`xfer_receives` is populated only by `AssetUploadRequest`), so the
  client's terrain upload (`Command::RequestRegionTerrainUpload`) has no
  server counterpart.

Add: a `ServerEvent::EstateOwnerRequest { method, params }` (or a typed
terrain variant) for the un-special-cased estate methods,
`SimSession::send_initiate_download(filename, data)` (registers the file
and offers it), `SimSession::request_xfer_upload(filename)` (a named
pull feeding a `ServerEvent::XferReceived`), loopback tests driving the
real client methods, and the two new `SESSION_FLOW_COVERAGE` rows.

**Done (2026-08-21).** Reference behaviour cross-checked against OpenSim's
`LLClientView` `terrain` dispatch, `EstateManagementModule.HandleTerrainRequest`
/ `HandleUploadTerrain` and `EstateTerrainXferHandler`.

- `EstateOwnerMessage` is now an unguarded arm: `telehub` and `terrain`
  decode to typed events (`TerrainDownloadRequested { viewer_filename }`,
  `TerrainUploadRequested { viewer_filename }`, `TerrainBakeRequested`);
  every other method — and an unknown sub-command of those two — surfaces
  as `ServerEvent::EstateOwnerRequest { method, invoice, params }` instead
  of the `ClientMessage` catch-all.
- `SimSession::send_initiate_download(sim_filename, viewer_filename, data)`
  registers the file and sends the `InitiateDownload` (agent id, both
  names); the client's existing auto-follow completes it.
- `SimSession::request_xfer_upload(filename) -> XferId` sends the named
  `RequestXfer` pull (nil `VFileID`, small packets, the
  `EstateTerrainXferHandler` shape); `SimXferReceive` gained a purpose
  (`AssetUpload` | `NamedFile`) and a completed named pull surfaces as
  `ServerEvent::XferReceived { xfer_id, filename, data }`.
- `SESSION_FLOW_COVERAGE` rows `terrain RAW download` / `terrain RAW upload`
  (`Mirrored`); loopback tests `terrain_download_round_trips`,
  `terrain_upload_round_trips` (incl. a client abort mid-pull) and
  `untyped_estate_owner_message_is_surfaced` drive the real client methods.
- The fake grid forwards events verbatim, so no driver change; scripting a
  terrain fixture stays with [[viewer-fake-grid-udp-assets]].
