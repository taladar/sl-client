---
id: test-fake-grid-rezzed-prim-texture
title: A prim the fake grid rezzes carries no TextureEntry
topic: test
status: ideas
origin: noticed doing test-assets-object-asset-codec (2026-09-06)
points: 1
refs: [test-fake-grid-object-write-path, test-assets-object-asset-codec]
---

Context: [context/testing.md](../context/testing.md).

`world::bare_object` leaves `texture_entry` empty and nothing on the rez path
(`ServerEvent::RezObject` → `prim_from_shape`) fills it in, so a prim a client
rezzes against the fake grid arrives with **no per-face data at all**. A real
simulator gives a new prim the default texture — the plywood
`89556747-24cb-43ed-920b-47caed15465f`, which is the very id the reference's
own object asset carries on all six faces.

It surfaced through the take: `store_taken_asset` has to state the faces the
shape renders, and `decode_texture_entry` of an empty blob is *no faces*, so
the take writes untextured faces and says so in a comment rather than letting
the asset claim the prim has none.

Two things to settle:

- whether a rezzed prim should get the default texture here (it should, if
  nothing renders differently for it — check the render baselines and the
  `object-edit` case, which sets a texture on a prim it rezzes);
- whether anything else reads a rezzed prim's texture entry and quietly gets an
  empty one.
