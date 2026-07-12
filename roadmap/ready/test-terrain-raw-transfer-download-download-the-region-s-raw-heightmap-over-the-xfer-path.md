---
id: test-terrain-raw-transfer-download
title: download the region's RAW heightmap over the Xfer path
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

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
