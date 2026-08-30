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

## How to settle it

[[viewer-water-transparency-scene-matrix]]: a synthetic scene puts a known
opaque marker behind a translucent face and asks whether the marker's colour
reaches the frame through it. That is decidable from a pixel and does not care
how bright the face is. Do that before touching any shader.
