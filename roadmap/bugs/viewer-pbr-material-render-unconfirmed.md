---
id: viewer-pbr-material-render-unconfirmed
title: Actually render PBR (GLTF) materials on a face — and on the material preview
topic: viewer
status: bugs
origin: user request (2026-07-25) while inspecting viewer-material-swatch-sphere-preview
refs: [viewer-face-materials-pbr, viewer-material-swatch-sphere-preview, viewer-pbr-material-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The PBR (GLTF) render-material pipeline decodes and maps correctly
([[viewer-face-materials-pbr]] — scalars, override composition, per-channel
maps, colour-space split), and the material **sphere preview**
([[viewer-material-swatch-sphere-preview]]) shades its sphere from the same
`MaterialManager::apply_preview` path. But an **on-screen render of a real PBR
material's texture maps** — on a world **face** and on the **preview sphere** —
is still **unconfirmed** on either reachable grid:

- **Local OpenSim** serves no PBR/GLTF render-material content at all, so
  nothing exercises the fetch/decode/apply path visibly.
- **aditi** does carry PBR builds and pushes real GLTF overrides (the override
  path is live-confirmed), but its
  **`ViewerAsset` capability persistently 503s** (see
  [[protocol-simulator-features-caps-503]]), so the base material asset and its
  texture **maps do not fetch** — faces render grey/untextured and the preview
  sphere shows only the factor scalars (base colour / metallic / roughness /
  emissive), never the maps.

So the *factor-only* half of a material renders (colour/metallic/roughness/
emissive on both face and sphere), but the *textured* half has never been seen.

**Do:** get a real PBR material with texture maps to render end-to-end — on a
face **and** on the preview sphere — by one of:

- provisioning an OpenSim prim with a GLTF material + fetchable maps (so the
  path is exercisable without aditi), and/or
- getting past aditi's `ViewerAsset` 503 (retry/alternate cap, or a spot/asset
  that does serve), and/or
- confirming with a locally uploaded material asset.

Then verify the maps land in the right slots / colour spaces on the drawn face
(base colour sRGB, normal + metallic-roughness linear, ORM red→occlusion) and
that the same maps appear on the preview sphere. This closes the P27.1
"LIVE-VERIFICATION GAP" note and the preview half the user flagged.
