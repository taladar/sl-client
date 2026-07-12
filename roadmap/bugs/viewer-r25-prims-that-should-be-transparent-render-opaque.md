---
id: viewer-r25
title: Prims that should be transparent render opaque
topic: viewer
status: bugs
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R25. Prims that should be transparent render opaque.** On aditi, some
plain prims that are transparent in Firestorm render fully opaque in the
viewer. Picked two on a live region — the **Mauve sign** and the **fence
around King Kong** — both plain **box** prims (`asset=None`,
`path_curve=16`/`profile_curve=1`), large and flat (`scale≈10×0.26×8` and
`0.24×9.77×2.92`), so this is a **prim**-face transparency path, not a
mesh/sculpt or an avatar bake. Candidate causes to
check: (1) a face whose **texture-entry tint alpha** is < 1 (the reference
viewer's per-face `blinn_phong_transparent`) is not driving the material's
`AlphaMode::Blend` — the prim face path only alpha-*masks* off a texture's own
alpha channel (R22d), and a genuinely translucent tint should blend; (2) a
face carrying an `LLMaterial` / GLTF **diffuse alpha mode** of `BLEND` (the
Phase-27 `legacy_alpha_override` / PBR material path) not being applied to a
prim face; (3) a **fullbright + alpha** or "alpha mode: alpha blending"
legacy-material face. Reproduce with the `P` pick tool on a known-glass prim
and log its `TextureFace` colour alpha + any material override before deciding
which path is dropping the transparency.
