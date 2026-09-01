---
id: test-fake-grid-builtin-textures
title: Serve the built-in sky, water and prim textures the viewer asks for
topic: test
status: done
origin: noticed live-verifying test-fake-grid-npc-avatars (2026-09-01)
points: 2
refs: [test-fake-grid-npc-avatars, viewer-static-asset-library]
---

Done (2026-09-01). Three pieces, because the ids and the pixels and the
grid that serves them each belong somewhere different.

**`sl-proto` owns the ids.** The eight UUIDs lived in four places — three
private constants in `sl-viewer-world-scene/src/sky.rs`, one in
`water.rs`, a literal in `sl-fake-grid`'s `blank_texture`, a string in
`sl-conformance`, and another in the texture picker — with no way for a
grid fixture to reach any of them. They are now
`DEFAULT_SUN_TEXTURE` / `DEFAULT_MOON_TEXTURE` / `DEFAULT_CLOUD_TEXTURE` /
`DEFAULT_RAINBOW_TEXTURE` / `DEFAULT_HALO_TEXTURE` /
`DEFAULT_BLOOM_TEXTURE` / `DEFAULT_WATER_NORMAL_TEXTURE` in
`sl-proto`'s `environment` module (with the
`BUILTIN_ENVIRONMENT_TEXTURES` list over them, which is what lets a test
prove none was forgotten) and `DEFAULT_PRIM_TEXTURE` in its `asset`
module, re-exported through `sl-client-bevy` and `sl-client-tokio`.
Every former copy now reads the shared constant, and
`sl-conformance`'s `plywood_texture()` stopped being fallible along the
way — a `Uuid` constant cannot fail to parse.

**`sl-test-assets::builtin` owns the pixels.** One generator per role,
built to be recognisable *in the role* rather than to look like Linden's
own: `sun_disc` / `moon_disc` (soft-rimmed discs on transparency, the
moon's surround the reference's own `<0x55,0x55,0x55,0x00>` that
`moonF.glsl` discards on), `cloud_noise` (a separable raised cosine, so
the blob reaches zero on all four edges and the sky tiles it at the half
dozen uv scales `cloudsF.glsl` samples it at), `rainbow_band` and
`halo_ring` (banded along the axis each shader actually samples — the
halo's bright band at `sin(22°)`, which is where `skyF.glsl` looks for
the 22° ring), `star_bloom` (a point on black, because the star field is
additive), `flat_wave_normal` and `plywood`. `library_textures()` returns
all eight as JPEG2000.

**The stock scenario serves them.** `default_assets` registers the eight
beside the four terrain detail solids, so a fake region's library is
twelve textures. The fake grid *is* a grid with a library, so a Linden
library id under its real UUID is honest here — the opposite call from
[[test-fake-grid-animation-assets]], which refused to let a fixture
animation wear a Linden animation id, for the opposite reason.

Two things the task did not ask for and got anyway, both because the
shared-id move made them free: the render scene's private constants are
gone (a viewer and a grid can no longer disagree about what "the default
sun" is), and the texture picker's **Default** quick choice picks the
same id everything else does.

Live-verified against the stock scenario with a **clean texture cache**
(`XDG_CACHE_HOME` pointed at a scratch dir — the first attempt proved
nothing, because the on-disk cache still held the *real* Linden moon from
an aditi session and answered before the network). With the cache empty:
zero `scheduling retry` / `gave up after` warnings on arrival, six of the
eight land in the fresh cache, and the moon renders as the pale stand-in
disc rather than as nothing. The remaining two — the wave normal and the
plywood — are served and fetchable
(`every_built_in_library_texture_is_fetchable`) but the viewer requested
neither during a stock or a `--catalogue` arrival; whether it *should*
is a viewer question, not a grid one, and no longer a failing fetch
either way.

Covered by `builtin::tests` (eight shape assertions and a
no-id-forgotten one), the fake grid's
`the_stock_assets_hold_every_builtin_library_texture`, and
`every_built_in_library_texture_is_fetchable`, which fetches all eight
over `GetTexture` through the real client stack and decodes each through
the viewer's own JPEG2000 decoder.

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
