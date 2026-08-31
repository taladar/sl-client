# Vendored Linden character assets

`character/` is a verbatim copy of the Second Life viewer's client-side
avatar assets — `avatar_lad.xml` (the visual-parameter table),
`avatar_skeleton.xml`, the base body meshes (`*.llm`), and the static
bake-layer textures (`*.tga`).

## Provenance

Copied from the Firestorm viewer source tree:

- checkout: `git@github.com:TommyTheTerrible/Firestorm-Vulkan.git`
  (a fork of `FirestormViewer/phoenix-firestorm`)
- revision: `d95dde1f8e96adabedea8234bc4aabcf8377f40c`
- path: `indra/newview/character/`

## Licence

These files are Linden Lab content distributed with the viewer sources
under the GNU Lesser General Public License, version 2.1 (see the
Firestorm tree's `doc/LICENSE-source.txt`). The licence text is
redistributed verbatim beside this file as [LGPL-license.txt]
(LGPL-license.txt), which is the condition under which the assets may
ship in this tree. "Second Life" and "Linden Lab" are registered
trademarks of Linden Research, Inc.

## Consumers

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

Do not edit the files under `character/` — they are upstream content,
kept byte-for-byte (the repo's text checks are skipped for them via
`.gitattributes`). To update, re-copy from a newer viewer checkout and
record the new revision here.
