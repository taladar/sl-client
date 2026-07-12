---
id: test-asset-upload
title: upload an asset via CAPS NewFileAgentInventory uploader
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 13 — Asset & texture pipeline `[both]`
---

Context: [context/test.md](../context/test.md).

`asset-upload` — upload an asset via CAPS `NewFileAgentInventory`
uploader. `1av`. (CAPS-only: the legacy UDP `AssetUploadRequest` client path
was **dropped** — modern viewers upload exclusively over the cap, which both
grids offer — mirroring `asset-fetch-http`. Removed `Command::UploadAssetUdp`,
`Event::AssetUploadComplete`, `Session::upload_asset_udp` /
`upload_asset_to_inventory_udp` / `advance_upload`, the `RequestXfer` /
`ConfirmXferPacket` / `AssetUploadComplete` incoming handlers, the
`AssetUploadRequest` + `SendXferPacket` senders, and the
`asset_uploads` / `upload_xfers` / `next_upload_id` /
`pending_inventory_uploads` / `pending_upload_callbacks` / `secure_session_id`
bookkeeping; the runtimes' `UploadAsset` now fails with
`Event::AssetUploadFailed` when the cap is absent. The generated
`AssetUploadRequest` / `AssetUploadComplete` / `SendXferPacket` wire codec is
kept for `SimSession` / trace, and the shared `Xfer` transport stays for its
other users (mute list, terrain RAW); see `comms/xfer.md`. No new client code
for the CAPS path (`Command::UploadAsset` already existed). The case uploads a
unique free notecard into the agent inventory root and asserts `AssetUploaded`
names a real asset id and inventory item, then deletes the item. Green on
OpenSim (store-asset + create-item in one step, ≈ 11 ms loopback). **Grid
divergence:** Second Life's `NewFileAgentInventory` accepts only file-upload
asset classes (texture / sound / animation / mesh / …) and answers a notecard
with `Invalid asset type` — on SL a notecard is created empty and its body set
over `UpdateNotecardAgentInventory` (see `notecard-create-update`) — so the
case records `partial` with the server's reason on aditi.)
