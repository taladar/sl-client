---
id: test-asset-fetch-http
title: HTTP ViewerAsset fetch through the caching AssetStore (sl-asset crate)
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 13 — Asset & texture pipeline `[both]`
---

Context: [context/test.md](../context/test.md).

`asset-fetch-http` — HTTP `ViewerAsset` fetch through the caching
`AssetStore` (`sl-asset` crate). `1av`. (Replaces the planned
`asset-transfer-udp`: the legacy UDP `TransferRequest` path was **dropped** —
modern SL only used it as a fallback when the `ViewerAsset` cap is absent, and
SL always offers it, so the UDP client path was removed in favour of the
HTTP `ViewerAsset` cap, which both grids expose. Green on OpenSim
(in-memory + on-disk cache proven); **partial** on aditi — its `ViewerAsset`
service persistently answers `503 Service Unavailable`, handled as a soft
failure.)
