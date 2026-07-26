---
id: viewer-perf-texture-decode-cache
title: Persistent decoded-texture cache (raw / BCn / HTJ2K transcode)
topic: viewer
status: ideas
origin: spin-off of the GPU-decode research (2026-07-26)
refs: [viewer-perf-gpu-jpeg2000-decode, viewer-texture-vram-budget,
  viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

Cache decoded textures on disk so the expensive classic-JPEG2000
decode happens at most once per asset + discard level; later sessions
skip OpenJPEG entirely. The reference viewer's texture cache plays
this role and is a large part of why revisited scenes load fast even
with a slow decoder. Spun off from the
[[viewer-perf-gpu-jpeg2000-decode]] research as the recommended
cheaper win — it removes most repeat decode cost regardless of which
decoder ultimately runs.

Sketch:

- Key: asset id + discard level. Texture assets are grid-global, so
  the cache is per-grid, not per-avatar; store it under the XDG cache
  root alongside the existing codestream handling in `TextureStore`
  (`sl-texture/src/store.rs`).
- Candidate on-disk formats to evaluate:
  1. Raw RGBA — simplest, largest (4 MB per 1024×1024 level).
  2. BCn in KTX2 — Bevy loads KTX2/BCn natively; 4:1–8:1 smaller than
     raw and stays compressed in VRAM, which also helps
     [[viewer-texture-vram-budget]]; costs a BCn encode after the
     first decode (can run at low priority).
  3. HTJ2K transcode via OpenJPH / the pure-Rust `openjph-core` crate
     — smallest on disk, keeps full quality, and re-decodes ~10×
     cheaper on CPU than classic J2C.
- Eviction / size budget as a preference (the reference viewer's
  texture cache defaults to ~1 GB).
- Interaction with the truncated-prefix fetch model: a cached coarse
  level must not block upgrading to a finer one — same coalescing
  rules as the in-memory store today (`drive`/`finest` in
  `sl-texture/src/store.rs`).
- Measure with the [[viewer-profiling]] tooling: revisit scene-load
  wall time should drop to near fetch-only levels.
