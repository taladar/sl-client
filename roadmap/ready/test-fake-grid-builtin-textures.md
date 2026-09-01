---
id: test-fake-grid-builtin-textures
title: Serve the built-in sky, water and prim textures the viewer asks for
topic: test
status: ready
origin: noticed live-verifying test-fake-grid-npc-avatars (2026-09-01)
points: 2
refs: [test-fake-grid-npc-avatars, viewer-static-asset-library]
---

Context: [context/testing.md](../context/testing.md).

Logging the viewer into the fake grid, eight texture fetches fail — each
then burning the full retry budget (`scheduling retry 1/6`, six times)
before it gives up, so the log noise buries anything else during arrival.

**Re-diagnosed (2026-09-01, while doing
[[viewer-static-asset-library]]).** This task previously called these
"the reference viewer's built-in default body / clothing textures" and
pointed at [[test-fake-grid-npc-avatars]]'s bake solution as the shape of
the fix. That was wrong on both counts: no avatar texture 404s at all,
and none of the eight ids is a wearable texture. Cross-referencing
Firestorm's `indra_constants.cpp`, `llsettingssky.cpp` and
`material_codes.cpp` against our own `sky.rs` / `water.rs`, they are:

- `32bfbcea-…` `DEFAULT_SUN_ID` — the sun disc (`sky.rs`);
- `d07f6eed-…` `IMG_MOON` — the moon disc (`sky.rs`);
- `1dc1368f-…` `DEFAULT_CLOUD_ID` — the cloud texture (`sky.rs`);
- `11b4c57c-…` `IMG_RAINBOW` and `12149143-…` `IMG_HALO` (`sky.rs`);
- `3c59f7fe-…` `IMG_BLOOM1` — the glow/bloom kernel (`sky.rs`);
- `822ded49-…` `DEFAULT_WATER_NORMAL` — the wave normal map (`water.rs`);
- `89556747-…` `LL_DEFAULT_WOOD_UUID` — the default prim texture, which
  every catalogue prim that names no texture of its own falls back to.

Firestorm marks the sky ones `// dataserver`: it ships none of them, and
[[viewer-static-asset-library]] confirmed its `static_assets` folders hold
only animations, wearables and gestures. So the viewer-side static library
cannot answer these, and the fix belongs on the grid side, where it always
did.

The fix is therefore the one this task always described, just aimed
correctly: register a procedural stand-in for each of the eight ids in
`scenario::default_assets`, next to the four terrain detail solids
already there. A fake grid *is* a grid with a library, so serving these
under their real ids is honest — unlike a fixture animation wearing a
Linden animation id, which [[test-fake-grid-animation-assets]] refused for
exactly the opposite reason.

They want to be recognisable rather than flat: a bright disc for the sun,
a paler one for the moon, a soft blob for the clouds, a flat `(128, 128,
255)` normal for the water, a wood-brown solid for the prim default. The
sun and moon in particular are alpha-masked discs in the reference, so a
solid square would read as a square sun.

Acceptance: an arrival against the stock scenario logs no failed texture
fetch; the sky's sun and moon are discs rather than missing textures.
