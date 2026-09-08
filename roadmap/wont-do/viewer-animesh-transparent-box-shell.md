---
id: viewer-animesh-transparent-box-shell
title: Animesh surrounded by an almost-transparent box shell
topic: viewer
status: wont-do
origin: user report during the p29-2 aditi verification (2026-07-23)
refs: [viewer-p29-2, viewer-r25]
---

Context: [context/viewer.md](../context/viewer.md).

Reported as: with the p29-2 fix in (the aditi Mario animeshes animate), each
Mario is **surrounded by an almost completely transparent box** that should
presumably not be visible at all.

**Not a defect. The box is a genuine 99%-transparent prim, and drawing it
faintly is correct.** Closed on the 2026-09-08 aditi run that finally
measured it instead of presuming.

## What it actually is

The `P` probe on three of its faces (2, 3, 4) reports them identically:

```text
pick face 2: texture=5748decc-f629-461c-9a36-a35a221fe21f
  repeats=(0.000,0.000) offset=(0.000,0.000) rot=0.000rad
  media_flags=0x00 texgen=0x00 planar=false
  color=[255,255,255,3] glow=0.000 material_id=None
pick face render: alpha_mode=Blend base_color_alpha=0.012 unlit=false
  textured=true shown=true fully_transparent=false
pick texture 5748decc-…: discard=0 current=32x32
pick: prim full_id=6740ffeb-… asset=None scale=(0.50,0.50,1.49)
  shape=PrimShapeParams { path_curve: 16, profile_curve: 1,
  path_begin: 9750, profile_hollow: 47500, … }
```

- A **prim**, not the animesh: `asset=None`, `profile_curve=1`
  (`LL_PCODE_PROFILE_SQUARE`) on `path_curve=16` (`LL_PCODE_PATH_LINE`) — a
  box — at `profile_hollow=47500` (95%, the maximum) and `path_begin=9750`.
  So 0.5 × 0.5 × 1.49 m with ~1.25 cm walls: the "shell" is literal
  geometry. It rezzes *before* the animesh because it has no asset to fetch.
- Tinted **white at alpha 3/255 = 1.2%** — 99% transparency, not 100%.
- Textured `5748decc-…` = **`TEXTURE_BLANK`**, the *opaque* white LSL
  library texture (`keywords_lsl_default.xml`). `TEXTURE_TRANSPARENT` is a
  different id (`8dcd4a48-…`), so no transparency comes from the texture.
- No legacy material, no glow, and it decoded (`textured=true`), so no
  fetch-failure or `LLMaterial` path is involved.

The reference's alpha-pool gate is `alpha > 0.f`, and `3 > 0`, so Firestorm
draws this face too. The user's verdict on seeing the numbers: "it doesn't
look very visible, just the kind of faint outline you notice when zooming
close" — which is what 1.2% coverage should look like. There is nothing to
fix.

## What the investigation did produce

- **[[viewer-animesh-transparent-box-shell]]'s leading hypothesis was
  wrong but found a real defect.** The reference adds a face at *exactly*
  100% transparency with no glow to no draw batch at all
  (`LLVOVolume::rebuildGeom`'s `if (alpha > 0.f || te->getGlow() > 0.f)`).
  This viewer built it like any other, and while its blend draw contributed
  nothing, Bevy's shadow pass gives an `AlphaMode::Blend` material only
  `MAY_DISCARD` — which discards nothing for blend — so a fully transparent
  prim cast a **solid shadow**. Now culled at face build
  (`objects::is_fully_transparent` → `Visibility::Hidden`), with the edit
  tool's pick and the face overlays kept working around it.
- **The `P` probe was answering with the selection outline.** Face overlays
  are parented onto the face and drawn slightly proud of it, so on anything
  selected the ray struck the overlay, no `PrimFaceEntity` was found, and
  none of the per-face lines printed — the probe was useless in exactly the
  situation it is reached for. It now filters those out, reaches a face the
  cull hid, and reports `shown` / `fully_transparent` / `textured`.
- **[[viewer-pbr-face-zero-alpha-not-culled]]** — the glTF half of the same
  cull, split out.
- **[[viewer-outline-swallows-thin-hollow-prim]]** — this box "lights up
  completely white" when selected, which is the inverted hull inflating a
  1.25 cm wall by 3.5% of the whole prim.

`repeats=(0.000,0.000)` was checked and is **not** a decode bug: the
reference packs `scale_s` as a raw `MVT_F32` (`llprimitive.cpp:1277`)
exactly as `decode_texture_entry` reads it, so the wire really carries zero
— a script passing `ZERO_VECTOR` for repeats. Invisible on this prim's
uniform white texture either way.
