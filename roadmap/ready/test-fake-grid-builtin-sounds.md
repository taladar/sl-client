---
id: test-fake-grid-builtin-sounds
title: The built-in UI sounds the viewer plays on arrival 404
topic: test
status: ready
origin: noticed live-verifying test-fake-grid-animation-assets (2026-09-01)
points: 2
refs: [viewer-static-asset-library, test-fake-grid-builtin-textures]
---

Context: [context/testing.md](../context/testing.md).

Arriving against the fake grid, `sl_viewer_platform::sound_cache` fails
six fetches — `104974e3-…`, `3d09f582-…`, `5e191c7b-…`, `77a018af-…`,
`a3f48b85-…`, `d7a9a565-…` — the built-in UI sounds a viewer plays for
its own events. A live grid serves them from its library; the fake grid
serves no asset it was not handed.

This task originally covered the built-in **animation** half as well
(`2408fe9e-…` `stand` and the rest). [[viewer-static-asset-library]]
closed that half: the viewer now ships Firestorm's 129 `.animatn` files
and every asset store answers from them before reaching the network, so
the animation fetches no longer happen at all.

The same answer is not available here. Firestorm ships **no** sounds —
its `static_assets` / `fs_static_assets` folders hold only animations,
wearables and gestures, and there is no `.wav`/`.ogg` anywhere in its
tree. So the bytes have to come from somewhere else:

- serve a synthetic sound per id from `scenario::default_assets`, the
  way [[test-fake-grid-builtin-textures]] proposes for the sky and prim
  textures — a fake grid *is* a grid with a library, so answering a
  library id is honest; or
- teach `sound_cache` that a known built-in with no asset is not worth
  retrying, which removes the noise without inventing bytes, and is
  probably wanted regardless of the above.

Whichever way, `sl-audio` needs a decodable container: the reference's
built-in sounds are Ogg Vorbis, so a synthetic one has to be too — a
short tone encoded once in `sl-test-assets`, beside the animation
encoder, rather than an empty blob that would fail to decode instead of
failing to fetch.

Acceptance: an arrival against the stock scenario logs no failed sound
fetch, and the UI sounds either play or are known-silent by design.
