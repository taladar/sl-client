# Vendored Linden viewer assets

Three directories of upstream content, all copied verbatim from a
Firestorm checkout:

- `character/` — the client-side avatar assets: `avatar_lad.xml` (the
  visual-parameter table), `avatar_skeleton.xml`, the base body meshes
  (`*.llm`), and the static bake-layer textures (`*.tga`).
- `static_assets/` — the fixed-UUID assets a **grid** would otherwise
  serve, named `<uuid>.<class>`: 118 `.animatn` built-in agent
  animations, 76 `.gesture`, 49 `.clothing` and 48 `.bodypart` library
  wearables.
- `fs_static_assets/` — 11 more `.animatn`, Firestorm's own additions.

## Provenance

Copied from the Firestorm viewer source tree:

- checkout: `git@github.com:TommyTheTerrible/Firestorm-Vulkan.git`
  (a fork of `FirestormViewer/phoenix-firestorm`)
- revision: `d95dde1f8e96adabedea8234bc4aabcf8377f40c`
- paths: `indra/newview/character/`,
  `indra/newview/app_settings/static_assets/`,
  `indra/newview/app_settings/fs_static_assets/`

## Licence

These files are Linden Lab content distributed with the viewer sources
under the GNU Lesser General Public License, version 2.1 (see the
Firestorm tree's `doc/LICENSE-source.txt`). The licence text is
redistributed verbatim beside this file as [LGPL-license.txt]
(LGPL-license.txt), which is the condition under which the assets may
ship in this tree. "Second Life" and "Linden Lab" are registered
trademarks of Linden Research, Inc.

## Consumers of `character/`

- `AvatarAssetLibrary::load` (`sl-viewer-kit/src/avatar_assets.rs`)
  parses the skeleton, the LAD table and the meshes; the viewer loads
  it at session start so avatars get real bodies instead of placeholder
  spheres, and the HUD screen gets its attachment-point table.
- The viewer defaults `--viewer-assets` / `SL_VIEWER_ASSETS` to this
  directory when it exists; an explicit flag or environment variable
  still overrides it.
- Tests that need real-avatar correctness (the fixture world's HUD
  routing, avatar geometry checks) load this directory instead of
  depending on an out-of-tree Firestorm checkout.
- The render tier's avatar scenes (`avatar-base-part`,
  `avatar-morphed-body` in `sl-viewer-world-scene/src/render_scene.rs`)
  default to it too, so the sweep renders the real skeleton, LAD morphs
  and base meshes with no environment set. `SL_VIEWER_ASSETS=mini` is
  the escape hatch back to `sl-avatar`'s 4-vertex fixture, for bisecting
  a verdict change between asset content and render path.

## Consumers of `static_assets/` and `fs_static_assets/`

- `sl_asset::StaticAssetLibrary` indexes both directories by the UUID
  each file is named for, and every `sl_asset::AssetStore` consults that
  library **before** its disk cache and before the `ViewerAsset`
  capability. So the built-in animations, library wearables and gestures
  reach whatever asks for them — the animation resolver, the wearable
  fetcher, anything later — with no consumer knowing the library exists.
- The viewer defaults `--static-assets` / `SL_VIEWER_STATIC_ASSETS` to
  these two directories, in this order (Linden first, Firestorm second,
  so an id in both resolves to Firestorm's, which is upstream's own
  precedence). `--no-static-assets` ships none, which is how to tell a
  grid-side asset problem from a vendored-copy one.
- `vendored_static_assets.rs` (`sl-viewer-kit/tests/`) decodes every
  file with the decoder for its class, so a bad re-copy fails a test
  rather than a login.

This mirrors what the reference viewer does:
`LLDiskCache::prepopulateCacheWithStatic` copies the same files into the
asset cache at start-up and puts their UUIDs on a purge skip list, so a
later fetch of one is answered locally and never reaches the network.
Firestorm seeds `static_assets` only in its OpenSim-capable builds
(Second Life's own grid serves them); we seed both always, because the
viewer also has to work against `sl-fake-grid`, which has no library at
all.

Do not edit the vendored files — they are upstream content, kept
byte-for-byte (the repo's text checks are skipped for them via
`.gitattributes`). To update, re-copy from a newer viewer checkout and
record the new revision here.
