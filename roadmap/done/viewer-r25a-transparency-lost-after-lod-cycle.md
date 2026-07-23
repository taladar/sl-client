---
id: viewer-r25a
title: Prim transparency lost again after a LoD / derender cycle
topic: viewer
status: done
origin: user report during the R25 aditi verification (2026-07-23)
refs: [viewer-r25]
---

Context: [context/viewer.md](../context/viewer.md).

**R25a.** With [[viewer-r25]] fixed and verified on first spawn (the aditi
Mauve sign and King Kong fence render transparent), both prims **pop back
in opaque** after moving away (low LoD) and returning. The initial-spawn
pipeline is right, so a re-entry path diverges from it. Candidates:

- **The prim-LoD rebuild** (`apply_prim_lod` → `despawn_prim_faces` +
  `spawn_prim_faces`): new face entities and fresh `StandardMaterial`s.
  In code review this re-runs the same tint → `face_alpha_mode` build and
  re-registers with the legacy-material manager (whose cache re-applies
  with the R25 guard), so it *should* come back transparent — verify at
  runtime whether it actually does, and whether ordering between the
  rebuild, `apply_prim_textures`, and `apply_legacy_materials` can leave
  the opaque state standing.
- **A derender / re-create cycle** rather than a LoD swap: if the object
  was killed and re-created from an `ObjectUpdateCached` / compressed
  update on return, the rebuilt `TextureEntry` may be missing the tint
  alpha or the `material_id` (either loses the transparency: an opaque
  tint with no material never enters the blend pass) — check the cached /
  compressed update paths' TE fidelity against the full-update path.

**Diagnose live with `P`** on the re-opaqued prim (the R25 pick dump):
`color[3]`, `material_id`, the resolved `alpha_mode`, and whether the
legacy material shows "not fetched/decoded" pin which case it is —
missing TE data (second candidate), missing material fetch, or an apply
that never re-ran (first candidate).

## Root cause (2026-07-23) — neither candidate above

An instrumented aditi session (`SL_VIEWER_LOG_LEGACY_MATERIALS`) showed
every registered face at `tint_a=255`: these prims' transparency is the
**texture's alpha channel** (the R22d resolution), not the tint — and the
loss coincided with the full-detail texture arriving, because that is
when the prim-LoD driver re-tessellates. `face_material` had two paths to
meet a texture: the *parked* path (texture not yet decoded → resolved by
`apply_prim_textures` on decode, upgrading an opaque face to Mask/Blend)
and the *immediate* path (texture already uploaded → image attached
directly) — and the immediate path **skipped the alpha resolution
entirely**. Any face rebuilt while its texture was resident (LoD
re-tessellation, shape change, derender/re-create) therefore came back
permanently opaque.

**Fixed** by extracting the R22d resolution into
`resolve_texture_alpha_mode` and calling it from both paths —
`face_material` now resolves the alpha of an already-resident texture at
build time. Unit-tested (`texture_alpha_resolution_upgrades_only_opaque_
faces`). **Verified live on aditi (2026-07-23): the fence and sign stay
transparent through the away-and-back LoD cycle.**

The pre-existing ordering race between the R22d upgrade and a legacy
material's `NONE`-mode apply (a face with *both* an alpha texture and a
`NONE` material rendered per whichever applied last) is **also fixed**,
per the user's call to make it deterministic: an applied legacy alpha
override marks the face material
(`LegacyMaterialManager::alpha_overridden`), and the R22d resolution
skips a marked face — so `NONE` over an alpha texture renders opaque
regardless of arrival order, as the reference does. (A *translucent*
tint still reports no override, keeping the R25 tint precedence intact.)
