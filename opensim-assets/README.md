# Vendored OpenSimulator library textures

The **standard library textures** a viewer fetches from any grid, copied
verbatim from an OpenSimulator checkout so `sl-fake-grid` can answer them
with the real pixels.

- `textures/` — 17 JPEG2000 textures named `<uuid>.j2c`: the fifteen
  standard bump maps of `std_bump.ini` (woodgrain, bark, bricks,
  checker, concrete, crustytile, cutstone, discs, gravel, petridish,
  siding, stonetile, stucco, suction, weave), plus `IMG_SMOKE` and
  `IMG_FACE_SELECT`.

## Why these are needed

The reference viewer builds its standard-bumpmap table from
`app_settings/std_bump.ini` **at startup** and fetches all fifteen over
`GetTexture` whether or not any face uses one. None ship with the
client. A grid that serves nothing for them leaves fifteen fetches
retrying on every arrival — which, besides rendering bumped faces flat,
is on its own enough to stop a scene ever falling quiet, so a capture
harness waiting on quiescence always times out instead of photographing
a settled scene.

## Provenance

Copied from an OpenSimulator checkout:

- checkout: `git://opensimulator.org/git/opensim`
- revision: `36f6d16a61`
- path: `bin/assets/TexturesAssetSet/`

Not everything a viewer asks for is in OpenSimulator's set. The two
water-plane textures (`2bfd3884-7e27-69b9-ba3a-3e673f680004`,
`43c32285-d658-1793-c123-bf86315de055`), the avatar sentinels
(`IMG_DEFAULT_AVATAR`, `IMG_INVISIBLE`) and the environment textures are
absent from it, so they are not vendored here.

## Licence

OpenSimulator's own notice for this directory is redistributed verbatim
beside this file as [licenses.txt](licenses.txt), and is the condition
under which these files may ship here. It lists five origins — the
Blender Texture Disk (public domain), the vterrain.org Hawaiian plant
textures (public domain), the Golgotha textures (public domain),
textures donated to the public domain by Babblefrog, and the VTerrain
Project (MIT) — and a sixth:

> From Second Life(TM) Viewer Artwork. Copyright (C) 2008 Linden
> Research, Inc. […] licenses the Second Life viewer artwork […] under
> the Creative Commons Attribution-Share Alike 3.0 License

The notice does not say which file belongs to which origin, so **treat
every file here as CC BY-SA 3.0**, the most restrictive of the six. That
carries two obligations, both of which this directory discharges: the
notice travels with the files (`licenses.txt`), and any change to the
originals must be identified. **The files are unmodified**, copied
byte-for-byte; if that ever stops being true, say so here.

"Second Life" and "Linden Lab" are registered trademarks of Linden
Research, Inc., and the notice's trademark reservation applies.

## Consumers

- `sl_test_assets::builtin` embeds them with `include_bytes!` and keys
  them by the ids `sl-proto` names (`BUILTIN_BUMPMAP_TEXTURES`,
  `BUILTIN_VIEWER_TEXTURES`), so nothing restates a UUID the renderer
  already knows.
- `sl_fake_grid::scenario::default_assets` puts them in every scenario's
  asset store, so any fixture — stock or `--catalogue` — answers them.

Do not edit the vendored files: they are upstream content kept
byte-for-byte, which is also what lets the licence claim above stand.
The repo's text checks are skipped for them via `.gitattributes`. To
update, re-copy from a newer OpenSimulator checkout and record the new
revision here.
