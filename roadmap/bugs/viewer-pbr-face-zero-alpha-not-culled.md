---
id: viewer-pbr-face-zero-alpha-not-culled
title: A PBR face at zero base-colour alpha is still built (the glTF half of the transparency cull)
topic: viewer
status: bugs
origin: split out while porting the alpha-pool cull (2026-09-08)
refs: [viewer-animesh-transparent-box-shell]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-animesh-transparent-box-shell]] ported the reference's
fully-transparent cull for a **legacy** face: `objects::is_fully_transparent`
reads the texture entry's tint alpha at face build and hides the face, so it
leaves the colour *and* shadow passes exactly as `LLVOVolume::rebuildGeom`'s
`if (alpha > 0.f || te->getGlow() > 0.f)` gate leaves it out of every batch.

The reference has the **same gate a second time** for a face carrying a glTF
render material, a few lines further down the same function:

```text
bool should_render = true;
if (gltf_mat->mAlphaMode == LLGLTFMaterial::ALPHA_MODE_BLEND)
{
    if (gltf_mat->mBaseColor.mV[3] == 0.0f && !LLDrawPoolAlpha::sShowDebugAlpha)
    {
        should_render = false;
    }
}
if (should_render) { add_face(sPbrFaces, pbr_count, facep); }
```

That half is not ported. A face whose glTF material is `BLEND` at base-colour
alpha `0` is still built, so it still casts the solid shadow the legacy fix
removed. The reference's own comment says it should in theory never be reached
("For rigged meshes, this apparently may not happen consistently"), so this is
parity rather than a live report — but the shadow it leaves is the same one.

Note the two gates are alternatives, not a conjunction: a PBR face's texture
entry tint is **not** what decides it. A face whose TE tint is transparent but
whose glTF base colour is opaque renders in the reference, and the build-time
legacy verdict would currently hide it.

## Why it is not a one-liner

The legacy verdict is reached at face build (`objects::spawn_face_entity`),
which has the face entity to write `Visibility` onto. The PBR decision is made
in `materials::apply_material_scalars`, reached from `recompose_face`, which
works from a `MaterialManager::FaceSlot` — a material id, a handle and a UV
transform, no entity. Every path that can change the verdict has to end at the
same place:

- `register_pbr_materials` / `refresh_face_material` — a material assigned;
- `recompose_face` — the asset decoding, or an override changing;
- `revert_face_to_diffuse` — the material cleared, which must hand the face
  **back** to the legacy tint verdict;
- `apply_blinn_phong_hide` — the FIRE-35138 preview, which renders the face's
  Blinn-Phong layer, so the legacy verdict applies while it is hidden.

Sketch: carry `entity` on `FaceSlot` (every registration site has it —
`register_pbr_materials` queries the face, `collect_linkset_pbr_faces` returns
it), have each of those paths push `(Entity, bool)` onto a queue on the
manager, and drain it into `Visibility` in one small system. The legacy
build-time write stays the default for a face with no PBR slot.

Rejected: one system re-deriving the verdict from the composed `FaceMaterial`
(`base_color.alpha() == 0 && alpha_mode == Blend && glow == 0`) would express
both gates once and self-correct on every path, but it has no cheap trigger —
a material recomposed in place does not mark its face's components changed, so
it would mean a full scan of every face entity per frame.
