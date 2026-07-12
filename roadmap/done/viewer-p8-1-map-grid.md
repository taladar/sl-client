---
id: viewer-p8-1
title: Map → grid
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 8 — `sl-sculpt` (sculpt-texture → geometry)
---

Context: [context/viewer.md](../context/viewer.md).

**P8.1. Map → grid.** The crate takes a decoded RGBA8 sculpt map
(`sl_texture::DecodedImage`) + `sculpt_type` / flags and returns
`sl_prim::PrimMesh`. Resample to a fixed working size (bilinear); pixel
`(r, g, b) / 255 - 0.5` → a grid vertex. The crate itself stays I/O-free
(like `sl-prim`): it never fetches or decodes. The `DecodedImage` it consumes
must be sourced from the shared `TextureStore` (the same fetch /
off-thread-decode / disk-cache pipeline the Phase 6 texturing drives), which
the viewer supplies at P9.1. Do not add an inline JPEG-2000 decode here.
Delivered as `tessellate(map, sculpt_type)` / `tessellate_with(map, params)`.
`sl-texture` is depended on with `default-features = false` so the pure crate
does not pull the OpenJPEG C dependency (only the `DecodedImage` type); the
fixed working grid is `WORKING_SUBDIVISIONS = 32` quad cells per side
(Firestorm's top sculpt LOD), bilinearly resampled per grid vertex.
