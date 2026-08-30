---
id: viewer-translucent-top-face-reads-opaque
title: A translucent prim's top face reads as opaque from above
topic: viewer
status: bugs
origin: user report, split from viewer-transparency-all-faces-skips-top (2026-08-30)
refs:
  [
    viewer-transparency-all-faces-skips-top,
    viewer-straddling-transparency-oit,
    viewer-water-transparency-scene-matrix,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

A box set to 50 % transparency renders translucent on five faces, but its
**top** face looks solid when seen from above. This is what
[[viewer-transparency-all-faces-skips-top]] originally reported as "the edit
skipped the top face"; that turned out to be false — the edit reaches every face
— so what is left is a rendering question, split out here.

## What has been ruled out

Measured on the local grid against the North Region box (3.30 m, centre 0.49 m
under the surface, plywood at 50 %):

- **The data.** The simulator's own stored `TextureEntry` for that prim has a
  single colour default of `0000007F` and no per-face exception — every face,
  the top included, is alpha 128.
- **The material.** `sl_viewer::texture_edit` shows all six faces built from
  `rgba[255, 255, 255, 128]`, `shared=true`: one interned `FaceMaterial` for the
  whole box, so the top face cannot differ from the five that look right.
- **The water bucket.** `SL_VIEWER_DISABLE_PRE_WATER_PASS=1` changes the top
  face's pixels by ~4/255 — it renders the same whether or not the pre-water
  split runs at all.
- **The glow pass, and the glow/alpha interaction in the face material.** There
  is a real one — `face_material.wgsl` writes the face's glow scalar into the
  fragment **alpha**, which the glow pass reads as its per-face mask, and
  `preserve_glow_mask_alpha` forces a blend pipeline's alpha blend to
  `(Zero, One)` so a transparent face does not overwrite that mask. If either
  half were wrong for a blend face its coverage would be clobbered, and a face
  whose coverage is replaced by `1.0` renders exactly this. But the shader gates
  the write on the alpha mode (`OPAQUE` or `MASK` only, so a blend face's
  coverage is left alone), and the live A/B agrees: with
  `SL_VIEWER_DISABLE_GLOW=1` the top face comes back within 1–3/255 of the
  glow-on capture. Not the glow.

  **Pin `SL_VIEWER_SKY_DAY_POSITION` for any such A/B.** The first attempt at
  this comparison showed a ~40/255 difference that was entirely the sun moving
  between the two runs, and it read exactly like a real result.

## What is not yet known

Whether it is genuinely drawing opaque, or blending correctly against a
background that happens to look like it. Looking down, what is behind the top
face is the sea, and behind that the prim's own submerged faces refracted
through it — plywood seen through water. In an HDR scene a sunlit face is well
above 1.0 before tone mapping, so a correct 50 % blend of a bright face over a
dark background still tone-maps bright. A photograph cannot separate those two;
an A/B against the same prim at alpha 255 can, and a synthetic scene with a
known background can.

Two observations that fit "correct blend, misleading contrast" and are worth
keeping together with this:

- The same cube "looks a lot more translucent when viewed against the underwater
  fog band" (user, 2026-08-30) — i.e. it reads as more transparent against a
  *bright* background, which is what a correct blend does and an opaque face
  cannot do.
- The top face is the one face pointing at the sky, so it is the most brightly
  lit of the six — exactly the face where the contrast illusion would be
  strongest.

## What the matrix says

[[viewer-water-transparency-scene-matrix]] now puts an opaque wall behind five
translucent boxes and classifies every band of every one from three eye heights.
**Every top cap comes back `translucent`** — drawn, and with the wall's colour
reaching the frame through it — from above the surface and from under it. So the
material, the phase and the blend are right for a horizontal half-transparent
face; there is no "top faces render opaque" fault in the pipeline.

That leaves the live picture. It is consistent with a correct blend whose
background happens to resemble the face (looking down, what is behind the cap is
the sea, and through it the prim's own submerged faces refracted — plywood seen
through water) at a contrast a photograph cannot resolve. It is **not proof** of
that, because the synthetic cap is backed by a wall rather than by the sea.

## How to settle it

The remaining question is narrow enough for one live A/B: capture the North
Region box from a fixed pose at alpha 128, set it opaque, capture again from the
same pose, and compare the top face's pixels. Identical means it really is
drawing opaque; different means it was blending all along and the eye was
fooled. `sl-repl`'s `set_object_image` can only send a whole-face default (it
takes a texture id and no tint), so either extend it to carry a colour or do the
second capture through the viewer's own Texture tab.

Do that before touching any shader — the matrix has already shown the shader is
not at fault for a top cap in general.
