---
id: test-terrain-raw-transfer-upload
title: upload a RAW heightmap over the Xfer path
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

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
