---
id: viewer-audit-decoded-texture-uploaders
title: Eight independent DecodedTexture uploaders each re-decide colour space
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

There are eight independent `DecodedTexture -> Image` uploaders:
`sl-viewer-world-api/src/lib.rs:6521`,
`sl-viewer-world-objects/src/legacy_materials.rs:213` and `:239`,
`materials.rs:700`, `bump.rs:347`, `sl-viewer-world-scene/src/water.rs:583`,
`sky.rs:1351`, `particles.rs:1213`,
`sl-viewer-world-objects/src/textures.rs:1713`.

They exist because the shared `sl_client_bevy::to_bevy_image`
(`sl-client-bevy/src/textures.rs:31`) hardcodes `Rgba8UnormSrgb` plus `Repeat`,
so every consumer needing linear or clamp forked it — and each fork re-decides
colour space and address mode **by convention**. This is the known
normal-maps-must-be-linear trap, now replicated eight ways.

Scope: one parameterised `upload_decoded(decoded, ColorSpace, AddressMode)` in
`sl-viewer-kit`, so the choice is a typed argument rather than a convention.

Worth doing in the same pass: `DecodedImage` (`sl-texture/src/decode.rs:18`)
records `components`, `discard_level`, `min_alpha` and `max_alpha` but **not**
whether its pixels are sRGB colour or linear data. A `ColorSpace` field set at
the decode site would make the project's own rule mechanically enforceable
instead of a convention each uploader re-derives.
