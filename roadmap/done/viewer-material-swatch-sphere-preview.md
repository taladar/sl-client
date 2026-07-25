---
id: viewer-material-swatch-sphere-preview
title: Render a material-on-a-sphere preview for the PBR material swatch
topic: viewer
status: done
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

Also fill the **material picker's preview pane** with the rendered sphere. When
[[viewer-inventory-materials-not-shown]] gave [[viewer-ui-texture-picker]] a
[`PickerKind::Material`] mode, the picker's single preview pane was left
**blank** in material mode (`handle_open_texture_picker` clears it and
`request_preview_texture` skips it — a material id is not a texture the pane can
decode). This task should render the *selected* material on the sphere and show
that image in the pane, so the picker previews a material the same way it
previews a texture.

Reference (Firestorm, read-only): `LLTextureCtrl` material-preview draw,
`llmaterialeditor` preview, `llfloatermaterialeditor`.

## Done (2026-07-25)

New viewer module `material_preview.rs`: a pool of offscreen **studios**, each
an isolated render layer holding a sphere, a key `DirectionalLight`, and a
camera rendering into a `RenderTarget::Image` (mirroring the HUD / probe
render-to-texture setups). A UI node opts in with a `MaterialPreview` component
(`Empty` / `Material(Box<GltfMaterial>)` / `Asset(AssetKey)`); a driver system
resolves it, binds a studio, shades the sphere, and points the node's
`ImageNode` at the studio image. Studios are pooled and rebound as previews come
and go, so the common case (the swatch + the picker pane) uses two.

- The render-material swatch (`edit_material.rs`) now previews the face's
  **effective** material (base + override, already folded by
  `sync_material_widgets`) as `MaterialPreview::Material`, replacing the
  base-colour-texture stand-in.
- The material picker's preview pane (`ui_texture_picker.rs`) previews the
  **selected** material by asset id (`MaterialPreview::Asset`), decoded through
  the `MaterialManager`; the pane is no longer blank in material mode.
- Sphere shading reuses the world PBR path via a new
  `MaterialManager::apply_preview` (scalars + the shared texture-patch queue
  that `apply_pbr_textures` fills), so a preview looks like the material does in
  world.

Verified: `cargo clippy`/tests clean (studio-pool bind/reuse + distinct-layer
unit tests). On-screen fidelity needs PBR material content to render (OpenSim
serves none; aditi carries PBR builds — the "`ViewerAsset`-503 wall" once noted
here was a misdiagnosis of rare transient 503s, corrected under
[[viewer-pbr-material-render-unconfirmed]]) — factor-only materials (base colour
/ metallic / roughness / emissive) render on the sphere regardless.
