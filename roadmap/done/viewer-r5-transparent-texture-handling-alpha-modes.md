---
id: viewer-r5
title: Transparent-texture handling / alpha modes
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R5. Transparent-texture handling / alpha modes.** `face_material` no
longer forces `AlphaMode::Opaque`: a face whose tint colour is non-opaque
blends, and a face whose texture carries an alpha channel (2- or 4-component
codestream) is upgraded to `AlphaMode::Blend` once it decodes — so the
Second Life world's many transparent surfaces (invisible prims, glass, sky-
platform floors) stop rendering as solid region-sized walls. Covers prim,
sculpt, and mesh faces; finishes the **eyelashes** (from R2b), which now show
with proper transparency. The precise legacy-materials `DiffuseAlphaMode`
(mask cutoff / emissive) and avatar-face alpha stay deferred. Also: the
all-`f` GLTF material-override null-texture sentinel
(`GLTF_OVERRIDE_NULL_UUID`) is now treated as "no texture" rather than
endlessly re-fetched (it 503s).
