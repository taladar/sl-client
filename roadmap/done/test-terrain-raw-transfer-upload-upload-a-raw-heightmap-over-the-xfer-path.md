---
id: test-terrain-raw-transfer-upload
title: upload a RAW heightmap over the Xfer path
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

**Done** (OpenSim green; `[opensim]`-only as planned). The client sends
`EstateOwnerMessage`/`terrain` `["upload filename", "terrain.raw"]`
(`Session::request_region_terrain_upload` /
`Command::RequestRegionTerrainUpload`) and registers the offered bytes; the
simulator answers with a `RequestXfer` naming that file and picking the
transfer id, which the session follows to stream the RAW up the new **Xfer
upload** path — one `SendXferPacket` at a time, each released by the previous
packet's `ConfirmXferPacket`, the first carrying the wire little-endian
size prefix and the last the high-bit end-of-file marker (mirroring the
reference viewer's `LLXferManager`). The final confirmation surfaces the new
`Event::XferUploaded`; a server `AbortXfer` surfaces the new
`Event::XferAborted`. The case downloads the current RAW, uploads a copy with a
band of points raised to a distinctive height (OpenSim only re-broadcasts
patches whose height actually changed), asserts the upload completes **and** a
land `TerrainPatch` re-broadcasts, then re-uploads the original to leave the
region clean. The recorded run streamed the `256 × 256` / 851 968-byte
heightmap up in ~13 s. Client-side unit tests in `sl-proto/tests/lifecycle.rs`
cover the `RequestXfer` → multi-packet stream → `XferUploaded` flow, the
unexpected-file guard, and the `AbortXfer` path.

`terrain-raw-transfer-upload` — upload a RAW heightmap over the `Xfer`
path. `[opensim] 1av` (estate owner). Sends `EstateOwnerMessage
"terrain"` with `["upload filename", <name>]`; the simulator answers with a
`RequestXfer` and the client streams the RAW back over the **Xfer upload**
path (`SendXferPacket`s driven by `ConfirmXferPacket`s). Asserts the region
terrain changes (the re-broadcast `LayerData` patch, as `modify-land`
observes). This is the case that pins the Xfer **upload** direction — its only
consumer once the legacy UDP asset upload is dropped (see `comms/xfer.md`).
Same estate-owner gating and `[opensim]`-only constraint. Needs new client
code: a generalised Xfer-upload send of a caller-supplied file (distinct from
the asset-upload trigger). To leave the region clean, download the current RAW
first and re-upload it, or restore with a `Revert` brush afterwards.
