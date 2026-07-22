---
id: viewer-hdri-environment
title: HDRI environment override
topic: viewer
status: ideas
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-environment-personal-lighting, viewer-phototools]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's HDRI preview: replace sky rendering and image-based
lighting with a loaded `.hdr`/`.exr` environment map (exposure + rotation
controls, irradiance-only mode) — a photography / product-shot tool
layered on the local environment override
([[viewer-environment-personal-lighting]]). Bevy's environment-map
lighting makes the IBL half nearly free; the sky-dome replacement and its
interaction with our EEP renderer is the real work. Park until the
photography cluster ([[viewer-phototools]]) creates the pull.

Reference (Firestorm, read-only): `RenderHDRI*` settings, the Develop →
Render Tests → HDRI Preview path.
