---
id: viewer-r25a
title: Prim transparency lost again after a LoD / derender cycle
topic: viewer
status: bugs
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
