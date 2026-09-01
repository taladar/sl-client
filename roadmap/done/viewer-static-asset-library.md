---
id: viewer-static-asset-library
title: Ship the viewer's static assets and answer every fetch from them first
topic: viewer
status: done
origin: asked while reviewing test-fake-grid-animation-assets (2026-09-01)
points: 5
refs: [test-fake-grid-animation-assets, test-fake-grid-builtin-sounds, test-fake-grid-builtin-textures]
---

Context: [context/testing.md](../context/testing.md).

A handful of asset UUIDs are fixed forever — the built-in agent
animations, the library body parts and clothing a default avatar is
assembled from, the library gestures. A live grid serves them from its
library, so the viewer *fetched* them; against a grid whose library is
incomplete (OpenSim), against `sl-fake-grid` (no library at all), or
before a region's `ViewerAsset` capability has arrived, it got nothing.

The reference viewer does not rely on the grid for these. Firestorm
keeps them as `<uuid>.<class>` under `app_settings/static_assets` and
`app_settings/fs_static_assets`, and
`LLDiskCache::prepopulateCacheWithStatic` copies them into the asset
cache at start-up with their UUIDs on a purge skip list, so a later fetch
is answered locally and never reaches the network.

Evaluate everything Firestorm ships that a grid would otherwise serve,
vendor it, and wire it up so the viewer and the tests find it.

Done (2026-09-01):

**What there was to take.** Firestorm's two folders hold 302 files in
exactly four grid asset classes: 129 `.animatn` (118 Linden + 11
Firestorm's own), 76 `.gesture`, 49 `.clothing`, 48 `.bodypart`. There is
nothing else of the kind in its tree — no sounds (`.wav`/`.ogg` do not
appear anywhere), and no textures: `app_settings/windlight/*`,
`app_settings/poses/`, `skins/`, `icons/` and `gltf/` are local settings,
UI art and source, not grid assets. `viewerart.xml` still maps
`avatar_*.bvh` names to the animation UUIDs, but the shipped asset is the
compiled `.animatn` — there is no `.bvh` in the tree.

**Where it went.** `viewer-assets/static_assets/` and
`viewer-assets/fs_static_assets/`, verbatim, beside the existing
`viewer-assets/character/`, from the same Firestorm revision the README
already records. 1.4 MB. `.gitattributes` marks both `binary` and
`-whitespace`: the wearable and gesture "text" formats carry trailing
NULs and upstream line endings no text check should normalise.

**How it is wired.** `sl_asset::StaticAssetLibrary` indexes the
directories by the UUID each file is named for (one `read_dir` each; no
file is read until something wants it), and `AssetStore::load_bytes`
consults it **ahead of both its disk cache and its fetcher**. So one
install serves every consumer — the animation resolver, the wearable
fetcher, anything later — with no consumer knowing the library exists.
That is the whole point: the alternative, a local-file path per manager,
is what was there before and it only ever covered animations.

The library is installed process-wide (`static_assets::install`, a
write-once `OnceLock`) because that is what the thing being modelled is:
assets that ship with the binary, the same for every store, reached by
managers that are constructed lazily from a Bevy `World` and have no
start-up options to be handed a library through. A store snapshots it at
construction, and `AssetStore::with_static_assets` takes one explicitly,
so nothing about this is untestable or reachable only through the global.

The viewer installs it in `run()` before anything builds a store, behind
`--static-assets <DIR>` / `SL_VIEWER_STATIC_ASSETS` (repeatable,
defaulting to the two vendored directories in Firestorm's own order —
Linden first, Firestorm second, so an id in both resolves to Firestorm's)
and `--no-static-assets`, which is how to tell a grid-side asset problem
from a vendored-copy one.

**What it replaced.** `AnimationManager` had a `viewer_assets` field and
looked for `<viewer-assets>/<uuid>.anim` before fetching. That path could
never fire: `--viewer-assets` points at `character/`, which holds no
`.anim`, and neither does a Firestorm install (its files are `.animatn`,
elsewhere). It is gone, along with the constructor argument;
`AssetStore::holds_static` replaces the "is there a local file?" test that
decided whether to park a request until the capability arrived.

**Deliberately not installed anywhere else.** `sl-repl` and
`sl-conformance` keep fetching from the grid: one is a wire-inspection
tool that must show what the grid actually served, the other asserts grid
behaviour. `sl-fake-grid` does not serve the library either — the viewer
answering locally is the fix, not the fake grid growing a library.

Six tests in `sl-viewer-world-avatar/tests/vendored_static_assets.rs` open
every vendored file with the decoder for its class — 129 through
`sl_anim::Motion::from_bytes`, 97 through `sl_avatar::WearableAsset::parse`,
76 gestures checked for their `LLMultiGesture` version line — and pin the
per-class inventory, so a partial re-copy fails there rather than as an
avatar that will not pose on a live grid. `sl-asset` covers the library
and the store hit itself, including that a shipped id never reaches the
fetcher.

Live-verified against the standalone fake grid: `shipping 302 static
asset(s)`, then `stand` (`2408fe9e-…`) resolving to **19 joint tracks**
and posing the own avatar — where before it was
`fetching animation 2408fe9e-… over ViewerAsset: asset not found` six
times over. Arrival's failed fetches went from 70 to 47, and every
remaining one is a texture or a sound, filed as
[[test-fake-grid-builtin-textures]] and [[test-fake-grid-builtin-sounds]].

Re-diagnosed on the way: the eight remaining texture 404s are **not** the
default wearable textures the old task file claimed. They are the sky and
water built-ins (`DEFAULT_SUN_ID`, `IMG_MOON`, `DEFAULT_CLOUD_ID`,
`IMG_RAINBOW`, `IMG_HALO`, `IMG_BLOOM1`, `DEFAULT_WATER_NORMAL`) plus
`LL_DEFAULT_WOOD_UUID`, the default prim texture — which is why
[[test-fake-grid-builtin-textures]] now carries that name.
