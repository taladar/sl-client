---
id: viewer-perf-bake-alpha-classify-offthread
title: Precompute alpha classification in the decode task
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: [viewer-perf-avatar-bake-apply-spikes]
---

Context: [context/viewer.md](../context/viewer.md).

Two frame-thread full-image pixel scans classified alpha after a texture had
already been decoded off-thread:

- `classify_bake_alpha` — a 512²+ scan per newly decoded avatar bake in
  `ensure_bake` (all bakes decoding in one frame scan in one frame; a crowd
  login is the burst case).
- `texture_has_transparency` — the same scan shape for prim/rigged-face
  textures deciding the transparent pass (bounded by the texture apply
  budget, but still per-image frame-thread scans).

Fix: `sl-texture`'s `DecodedImage` now carries `min_alpha` / `max_alpha`,
computed by one pass (`alpha_range`) **where the pixels are produced** — the
J2C decode task, `downsample`, and `sl-bake`'s composite (already on the
compute pool via `run_local_bake_job`). A new `DecodedImage::new`
constructor computes the range so no construction site can forget it (all
struct-literal sites across sl-texture / sl-bake / sl-sculpt / the viewer
converted). The frame-thread classifiers become O(1):

- `classify_bake_alpha(&decoded)`: `min_alpha >= 51` → Opaque (min checked
  first so an empty image's `(255, 0)` range classifies opaque),
  `max_alpha < 51` → Transparent, else Masked — same thresholds
  (`BAKE_ALPHA_CUTOFF` = reference `sMinimumAlpha` 0.2), unit tests updated
  to build real `DecodedTexture`s.
- `texture_has_transparency`: `has_alpha && min_alpha < 128`.

Verify: Tracy per-event max of `apply_avatar_bake_textures` /
`ingest_avatar_bakes` during a crowd login drops to allocation + `images.add`
cost; sl-texture / sl-bake / sl-sculpt test suites pass unchanged.
