---
id: test-server-appearance-bake
title: trigger a modern **server-side** appearance bake over the UpdateAvatar
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 13 — Asset & texture pipeline `[both]`
---

Context: [context/test.md](../context/test.md).

`server-appearance-bake` — trigger a modern **server-side** appearance
bake over the `UpdateAvatarAppearance` capability (SL "Sunshine" / central
baking). `1av`. The SL-native counterpart of `baked-texture-upload`: instead
of the client compositing and uploading each baked layer, the viewer only
POSTs `{ cof_version }` and the grid bakes the Current Outfit Folder, then
broadcasts the result over UDP `AvatarAppearance`. The case asserts on the
capability's own reply (`Event::ServerAppearanceUpdate`, the grid accepting
the bake), not the downstream broadcast (which reaches only *other*
observers). It drives the documented **COF-version handshake**: it starts from
version 0 and, on the grid's `success = false` / `expected = <n>` mismatch
reply, re-requests with `<n>` until the bake is accepted — needing no prior
inventory crawl to learn the current version. The
`RequestServerAppearanceUpdate` command and its reply decode were already
built; this is the first case to exercise them. **Grid divergence** (the
mirror of `baked-texture-upload`):
`complete` on aditi, where the handshake resolves in two attempts to the live
COF version (~15) and the bake is accepted in ≈ 1 s; `partial` on OpenSim,
which has no `UpdateAvatarAppearance` capability at all (it uses the legacy
client-side bake path) — recorded "capability not offered". `[both]`.
