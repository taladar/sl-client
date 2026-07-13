---
id: test-terrain-raw-transfer-download
title: download the region's RAW heightmap over the Xfer path
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

**Done** (OpenSim green; `[opensim]`-only as planned). The client sends
`EstateOwnerMessage`/`terrain` `["download filename", "terrain.raw"]`
(`Session::request_region_terrain_download` /
`Command::RequestRegionTerrainDownload`); the simulator answers with an
`InitiateDownload`, which the session follows automatically (new
`XferPurpose::ServerInitiated`, mirroring the reference viewer's
`process_initiate_download`) to stream the RAW back over the `Xfer` **download**
path, surfaced as the new `Event::ServerFileDownloaded`. The recorded run got a
`256 × 256` heightmap of 851 968 bytes (256² × the 13-byte LL RAW point stride)
in ~50 s — reliable `Xfer` is strictly one-packet-at-a-time, so the case uses a
240 s download timeout. Client-side unit tests in `sl-proto/tests/lifecycle.rs`
cover the `InitiateDownload` → `Xfer` → `ServerFileDownloaded` flow and the
other-agent guard.

`terrain-raw-transfer-download` — download the region's RAW heightmap over
the `Xfer` path. `[opensim] 1av` (estate owner). Sends `EstateOwnerMessage
"terrain"` with the `["download filename", <name>]` strings; the simulator
serialises the region heightmap to an LL RAW file and streams it back over the
**Xfer download** path, surfaced once reassembled. Asserts a non-empty RAW
terrain file of a plausible size arrives. Region-owner/god gated (Firestorm
enables the button only for `owner_or_god`), so runs as `--avatar
estate-owner`, who owns the local Default Region. `[opensim]` only — we own no
region on aditi (same constraint as the other estate-owner cases). Exercises
the Xfer download direction for a non-mute-list consumer; likely needs new
client code (the `"terrain"` `EstateOwnerMessage` + a terrain `XferPurpose`).
