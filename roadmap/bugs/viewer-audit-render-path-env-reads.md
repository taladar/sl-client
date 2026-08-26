---
id: viewer-audit-render-path-env-reads
title: About a dozen getenv calls and allocations per frame in the sky and post chain
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`day_position` (`sl-viewer-world-scene/src/sky.rs:1988`) calls
`std::env::var("SL_VIEWER_SKY_DAY_POSITION")` — allocating a `String` and taking
the process env lock — on **every invocation**, and it is called seven times per
frame (`sky.rs:707`, `:945`, `:1191`, `:1421`, `terrain.rs:94`, `water.rs:321`,
`underwater_fog.rs:153`).

Add `sky_linearize` / `resolved_sky_hdr_scale` per params build,
`exposure.rs:402` (twice up front plus once per camera at `:440`),
`tonemap.rs:205`/`:210`/`:215` and `glow.rs:146`/`:151` — the last two **inside
per-camera loops** — and it is roughly a dozen env reads per frame.

The workspace already has both fixes: `OnceLock` (`physics.rs:2312`) and
`Local<Option<bool>>` (`particles.rs:698`). Resolve each of these once.
