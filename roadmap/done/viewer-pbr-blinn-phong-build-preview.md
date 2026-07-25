---
id: viewer-pbr-blinn-phong-build-preview
title: PBR supersedes Blinn-Phong on a face; build tool previews Blinn-Phong (FIRE-35138)
topic: viewer
status: done
origin: user request (2026-07-25) while investigating viewer-pbr-material-render-unconfirmed
refs: [viewer-pbr-material-render-unconfirmed, viewer-face-materials-pbr, viewer-pbr-material-editor]
---

Context: [context/viewer.md](../context/viewer.md).

Two related behaviours around a face that carries **both** a legacy Blinn-Phong
`TextureEntry` layer and a PBR (GLTF) render material.

## 0. PBR did not render on an existing prim at all

The most visible defect: assigning a PBR render material to a prim already in
the scene (the build tool, or an in-world retexture) left the prim rendering
Blinn-Phong — the material loaded fine in the build-window swatch/sphere preview
but never reached the prim. Cause: a material assignment refreshes the object's
`ObjectRenderMaterials` holder **without re-tessellating its faces** (no shape /
`TextureEntry` change), so the `Added<PrimFaceEntity>`-gated
`register_pbr_materials` never saw the faces. Fixed with
`register_changed_render_materials`, a `Changed<ObjectRenderMaterials>`
registration path that (re)registers a holder's face children and recomposes
only the faces whose material actually changed
(`MaterialManager::refresh_face_material` returns `false` on an unchanged echo,
so a moving prim's per-update holder refresh costs a lookup, not a
recomposition; it preserves the face's captured diffuse `base_uv` so a later
change never double-applies a `KHR_texture_transform`).

This — not any aditi `ViewerAsset` 503 — was the real reason PBR "never rendered
on a face"; the 503 story in the older roadmap notes was a misdiagnosis of rare
transient 503s (corrected in [[viewer-pbr-material-render-unconfirmed]] and
`context/viewer.md`).

**Not yet handled:** a render material *cleared* in-world removes the holder
(`RemovedComponents<ObjectRenderMaterials>`); reverting those faces back to
Blinn-Phong is a follow-up (they currently keep their last PBR composition).

## 0b. Live-preview a browsed material on the prim (before OK)

The reference previews the *highlighted* material on the prim as you scroll the
*Pick: Material* list, and reverts on Cancel — ours only showed it after OK. The
picker already emits a **non-final** `TexturePicked` on each selection (and the
opened-on id on Cancel / X-close); `preview_pbr_material_picked` now applies
that to the selection's faces as a **no-wire** preview
(`MaterialManager::preview_face_material`), walking each node's own prim faces
(`prim_faces_of_node` — *all* faces, not only those already carrying a material,
since assignment can add one). A non-nil id composes it as the face's base PBR
material; the nil id a Cancel carries for a face that had none reverts it to
Blinn-Phong. OK still sends the assignment for real
(`apply_pbr_material_picked`), and the sim echo reconciles idempotently.

## 1. PBR supersedes the Blinn-Phong layer

The user saw the Blinn-Phong diffuse texture rendering *through* a PBR material.
Cause: `request_material_textures` left a PBR slot the material did not define
showing the face's leftover legacy diffuse/normal texture (the old "P27.1
fallback"). So a **factor-only** PBR material (base colour / metallic /
roughness factors, no texture maps) rendered its factor *multiplied over* the
Blinn-Phong diffuse — a mix that is neither.

Fix (mirrors the reference `LLTextureEntry::getGLTFRenderMaterial`): a PBR
render material now **fully supersedes** the face's Blinn-Phong layer. Any slot
the material does not name is *cleared* (`PbrSlot::fetchable_texture` → `None` ⇒
clear), so a factor-only material shows its factor alone. And while a PBR
material is still fetching (or turned out unavailable), the glTF **default**
material stands in (`recompose_face` uses `GltfMaterial::default()`), exactly as
the reference renders a not-yet-loaded `LLFetchedGLTFMaterial` — a PBR face
never falls back to its Blinn-Phong look on its own.

## 2. FIRE-35138 Blinn-Phong build-tool preview

Firestorm lets a face hold PBR and Blinn-Phong at once and, while the Build
Tools Texture tab is on the **Material (Blinn-Phong)** mode, *hides* the GLTF
render material on the **selected** objects so they render Blinn-Phong for
editing (`LLSelectMgr::hideGLTFMaterial` / `showGLTFMaterial`, saved+restored
per object, gated by `isSelected()`); switching to the PBR tab, deselecting, or
closing the floater restores the PBR material.

Ported as `materials::apply_blinn_phong_hide`. **Deliberate divergence** (the
workspace's prefer-maximal-scope rule): the reference hides only the selected
*prims*, but we hide the whole **linkset** — the user's rationale is that you
cannot judge a multi-prim build (a house's walls + floor + roof) with one wall
in Blinn-Phong beside PBR everything else. Because a Select-Face / edit-linked
selection carries the clicked *part* (whose entity subtree holds only its own
faces), each selected node is first resolved to its linkset **root** entity
(`ObjectState::linkset_root_of` → `entity_by_scoped`) and the hide walk runs
from there, so sibling prims are included — without that, only the clicked prim
flipped.

Implementation: the face keeps its one stable `StandardMaterial` handle
throughout (every other system that reads it — pick, bump, edit preview, LoD —
is unaffected); only its composition is swapped, between the PBR material
(`recompose_face`) and the Blinn-Phong material (the new
`textures::compose_face_material`, the in-place half of `face_material`). A
hidden face sits in `MaterialManager::hidden`, which suppresses its PBR
recomposition so a material / override / map arriving mid-edit cannot overwrite
the preview; parked PBR patches are dropped on hide and parked Blinn-Phong
diffuse on restore so neither pipeline's late texture lands on the other's
composition.

Client-side unit tests cover the precedence rule (factor-only ⇒ every slot
clears; only a named slot is requested; nil / override-null sentinel ⇒ clear).

**Known limitation:** the Blinn-Phong *preview* rebuilds only the diffuse
(+ tint / UV / surface flags); a face that also carries a legacy `LLMaterial`
normal/specular map does not show it in the preview, because
`register_legacy_materials` skips PBR faces so that material is never fetched.
Most PBR content's Blinn-Phong layer is diffuse-only, so this is rarely visible;
fetching legacy materials for PBR faces too is a possible follow-up.

**Live-verified on aditi (2026-07-25):** assigning a PBR material renders it on
the prim (was Blinn-Phong before the registration fix); browsing
*Pick: Material* live-previews each material on the prim and reverts on Cancel;
toggling Material / PBR flips the whole selected linkset Blinn-Phong ↔ PBR. This
also closes the on-screen-render confirmation tracked in
[[viewer-pbr-material-render-unconfirmed]].
