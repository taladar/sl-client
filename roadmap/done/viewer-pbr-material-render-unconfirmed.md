---
id: viewer-pbr-material-render-unconfirmed
title: Actually render PBR (GLTF) materials on a face — and on the material preview
topic: viewer
status: done
origin: user request (2026-07-25) while inspecting viewer-material-swatch-sphere-preview
refs: [viewer-face-materials-pbr, viewer-material-swatch-sphere-preview, viewer-pbr-material-editor, viewer-pbr-blinn-phong-build-preview]
---

Context: [context/viewer.md](../context/viewer.md).

The PBR (GLTF) render-material pipeline decodes and maps correctly
([[viewer-face-materials-pbr]] — scalars, override composition, per-channel
maps, colour-space split), and the material **sphere preview**
([[viewer-material-swatch-sphere-preview]]) shades its sphere from the same
`MaterialManager::apply_preview` path — and the user confirms **PBR materials
load fine in the build-window preview on aditi**.

**Correction (2026-07-25):** an earlier version of this file blamed the missing
on-screen render on aditi's `ViewerAsset` cap "persistently 503-ing". That was
**wrong** — out of dozens of runs only one or two textures 503'd once or twice;
the cap works and materials fetch + decode fine. The actual defects were in the
**client**:

1. A PBR render material **assigned to an existing prim** (the build tool, or an
   in-world retexture) refreshes the object's `ObjectRenderMaterials` holder
   **without re-tessellating its faces**, so the `Added<PrimFaceEntity>`-gated
   `register_pbr_materials` never saw the faces and the material was never
   applied — the prim kept rendering Blinn-Phong. Fixed in
   [[viewer-pbr-blinn-phong-build-preview]] by
   `register_changed_render_materials` (a `Changed<ObjectRenderMaterials>`
   registration path).
2. The Blinn-Phong layer bled *through* a factor-only PBR material — also fixed
   there (PBR now fully supersedes the legacy layer).

**Confirmed (2026-07-25):** with the registration fix in place, PBR materials
render end-to-end on a real prim on aditi — assigning a material shows it on the
face, and browsing the *Pick: Material* list live-previews each material on the
prim (the maps fetch fine; the "503 wall" story was wrong). The user verified
the on-screen render directly. Closed together with
[[viewer-pbr-blinn-phong-build-preview]].
