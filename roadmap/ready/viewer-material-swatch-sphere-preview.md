---
id: viewer-material-swatch-sphere-preview
title: Render a material-on-a-sphere preview for the PBR material swatch
topic: viewer
status: ready
origin: user request (2026-07-24) while testing the PBR material editor
refs: [viewer-face-materials-pbr, viewer-pbr-material-editor, viewer-ui-texture-picker]
---

Context: [context/viewer.md](../context/viewer.md).

The build Texture tab's PBR **render-material swatch** (and the material
picker's selection thumbnails) currently paints only a *texture* thumbnail via
[[viewer-ui-texture-picker]]'s `TextureSwatchValue`. A material asset id is not
a texture, so it renders blank; as a stand-in the swatch now shows the
material's
**base-colour (albedo) texture** ([[viewer-face-materials-pbr]]), which is blank
for a factor-only material and does not convey metallic / roughness / emissive.

The reference renders the material **on a lit sphere** (the LLTextureCtrl
material-preview / material-editor preview). Implement that: an offscreen sphere
mesh shaded with the effective GLTF material (base + override) rendered to a
`RenderTarget::Image`, the resulting image used as the swatch (and
material-picker row) thumbnail, refreshed when the material or its override
changes. Likely a
small dedicated camera + sphere + light on an isolated render layer, mirroring
the HUD/portrait render-to-texture setups already in the viewer.

Reference (Firestorm, read-only): `LLTextureCtrl` material-preview draw,
`llmaterialeditor` preview, `llfloatermaterialeditor`.
