---
id: viewer-r9
title: Planar texgen, unconfirmed
topic: viewer
status: bugs
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R9. Planar texgen, unconfirmed** (`TEX_GEN_PLANAR`).
A flat, solid, uncut disk (a full cylinder) still looked wrong
versus the reference viewer even though its cap is tessellated correctly
(a fan with **exactly affine** UVs, which by the affine-interpolation property
render the texture perfectly flat whatever the triangulation — proven, not a
tessellation bug). The suspected cause is **texture-gen mode**: a face's
`media_flags & 0x06` selects the UV source, and builders commonly set
architectural prims to **planar** mapping (`TEX_GEN_PLANAR`, `0x02`). The
reference viewer then ignores the volume's stored UVs and projects each
vertex's texture coordinate from its position (scaled by the object size) and
normal (`LLFace::planarProjection`); we always used the stored UVs. A
candidate fix is implemented but **the live visual bug is not yet confirmed
fixed**: `TextureFace::is_planar_texgen` (`sl-proto`), a `planar_texgen_uv`
port (`sl-client-bevy`, unit-tested against hand-computed reference values),
and `apply_planar_texgen` in the viewer — for a planar face it overwrites the
built mesh's UV0 with the projection (positions × object scale, same `1 - v`
flip as the stored UVs), keeping the per-face repeats/offset/rotation on the
material's `uv_transform` afterwards (the reference viewer's
`planarProjection` then `xform` order). Wired through prims, sculpts, and
(unrigged) meshes.
Worn **rigged** mesh attachments are not yet covered. **Open until verified in
the running viewer against the reference viewer** — the fix may be incomplete
or the real cause may differ.
