---
id: server-asset-service
title: Asset service — grid-wide content store
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-caps-assets]
---

Context: [context/server.md](../context/server.md).

The grid-wide, effectively immutable, content-addressed asset store:
textures, meshes, sounds, animations, notecards, scripts (source +
compiled state?), wearables, settings, materials.

- **Fetch path**: backs the per-region asset caps
  ([[protocol-sim-caps-assets]] builds the wire layer) — simulators
  either proxy or grant caps URLs that resolve here; HTTP ranges for
  progressive texture/mesh delivery; a caching/CDN tier in front for
  scale (SL fronts asset delivery with a CDN).
- **Store path**: upload ingestion from the caps upload flows and the
  legacy UDP path, type validation/size limits, quota hooks.
- **Semantics**: assets are write-once (new versions are new UUIDs);
  deletion/garbage collection policy is a real design question
  (OpenSim mostly never deletes; SL grid GC is opaque).
- Storage backend: filesystem/object-store + metadata DB; dedup by
  content hash is worth considering from day one.
