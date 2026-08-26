---
id: viewer-audit-texture-align-material-channels
title: Align planar faces does not propagate to the normal and specular transforms
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-edit/src/edit_texture_align.rs:129-137` writes only
`dst.scale_s` / `scale_t` / `offset_s` / `offset_t` / `rotation` — the diffuse
channel. The reference applies the same aligned values to all three:
`fspanelface.cpp:1442-1449` calls `setNormalRotation`, `setSpecularRotation`,
`setNormalOffsetX/Y` and `setNormalRepeatX/Y`.

The codebase already models these channels
(`sl-viewer-world-objects/src/legacy_materials.rs:489-496`,
`sl-viewer-edit/src/edit_material.rs:211-220`), so a bump-mapped face keeps an
unaligned bump after Align.

The core arithmetic is a faithful port of `llface.cpp:1090-1110`. Two further
divergences in the same file, lower confidence and worth confirming against the
reference before changing: `:93` picks the lowest-indexed selected face as the
anchor where the reference uses `LLSelectedTE::getFace(last_face, ...)`
(`fspanelface.cpp:1609`), and `:62-80` bails on anything but a single primary
`PRIMITIVE`, where the reference runs `applyToTEs` across the whole selection —
its Align Textures button is in fact only enabled when `getObjectCount() > 1`
(`:1903`), so multi-object planar align is missing entirely.

`edit_texture_align.rs` is 243 lines with three pure functions and zero tests:
`face_projection` (`:160`), `face_tangent` (`:211`), `wrap_unit` (`:241`).
