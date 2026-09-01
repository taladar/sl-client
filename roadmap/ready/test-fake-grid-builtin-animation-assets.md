---
id: test-fake-grid-builtin-animation-assets
title: The built-in animations and UI sounds the own avatar plays 404
topic: test
status: ready
origin: noticed live-verifying test-fake-grid-animation-assets (2026-09-01)
points: 2
refs: [test-fake-grid-animation-assets, test-fake-grid-default-wearable-textures]
---

Context: [context/testing.md](../context/testing.md).

Logging the viewer into the fake grid, arrival logs a failed fetch for
every built-in asset the own avatar needs and the grid was never handed:

- one animation — `2408fe9e-…` (`stand`), from
  `sl_viewer_world_avatar::animations`;
- six sounds — `104974e3-…`, `3d09f582-…`, `5e191c7b-…`, `77a018af-…`,
  `a3f48b85-…`, `d7a9a565-…`, from `sl_viewer_platform::sound_cache`.

`AnimationResolver::request` skips a *procedural* built-in without a
fetch, but a built-in the registry marks downloadable is a real Linden
asset a live grid serves from its library — and the fake grid serves no
asset it was not handed. The avatar therefore stands unposed, the UI is
silent, and the noise buries anything else during arrival, exactly as the
default wearable textures do in
[[test-fake-grid-default-wearable-textures]].

Unlike that one, the fix is **not** "register a fixture under the Linden
id": a synthetic motion served under `2408fe9e-…` would make the fixture
pretend to be a Linden asset, which [[test-fake-grid-animation-assets]]
deliberately avoids.

The real bytes do exist locally, though, in the checked-out Firestorm
tree: `indra/newview/app_settings/static_assets/<uuid>.<ext>` holds 291
files — 118 `.animatn` (all of them the modern `1.0` keyframe-motion
encoding `sl-anim` decodes, including `2408fe9e-…`), plus `.gesture`,
`.clothing` and `.bodypart`. It does **not** hold the sounds, and it ships
no `.bvh`: `app_settings/viewerart.xml` still maps the `avatar_*.bvh`
names to their UUIDs, but the shipped asset is the compiled `.animatn`.

So two options, to be decided when the task is picked up:

- point the viewer at those files — it already prefers
  `<viewer-assets>/<uuid>.anim` over the cap
  (`sl-viewer-world-avatar/src/animations.rs`), so this is a documented
  provisioning step (copy or symlink Firestorm's `static_assets`,
  renaming `.animatn` to `.anim`) rather than fake-grid work; or
- have the fake grid answer a *known built-in* id out of an opt-in asset
  directory the operator points it at, so a run with no directory logs
  one line instead of seven failed fetches.

Either way the sounds need their own answer — nothing in the workspace or
in Firestorm's tree carries them, so the honest options there are a
synthetic tone under a fixture id or simply not asking.

Acceptance: an arrival against the stock scenario does not log a failed
fetch per built-in, and whichever path is chosen is written down where a
manual viewer session will find it.
