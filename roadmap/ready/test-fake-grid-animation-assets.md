---
id: test-fake-grid-animation-assets
title: A synthetic animation asset the fake grid can actually serve
topic: test
status: ready
origin: test-fake-grid-npc-avatars follow-up (2026-09-01)
points: 3
refs: [test-fake-grid-npc-avatars, viewer-fake-grid-render-catalogue]
---

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-npc-avatars]] gives the catalogue's NPC an
`AvatarAnimation` naming the built-in `stand`, but nothing in the fake
grid serves an animation **asset** — `sl-test-assets` has no encoder for
one and the only `.anim` in the workspace is `sl-anim`'s 175-byte
`tests/fixtures/minimal.anim`, which belongs to that crate's tests.

So a viewer against the fake grid records the NPC as playing an animation
and then falls back to its own idle, and the render-catalogue check that
"two captures a second apart differ" has nothing to move.

Add a keyframe-motion **encoder** to `sl-test-assets` (`anim::` beside
`mesh::`), the inverse of `sl-anim`'s decoder: the version-1 header
(priority, duration, ease in/out, loop, hand pose), one joint with a
couple of rotation keyframes far enough apart to be visible, no
constraints. Round-trip it through `sl_anim` in a unit test — the decoder
is the contract, exactly as the mesh encoder is pinned by `sl-mesh`.

Then register it in the catalogue's assets under the animation id the NPC
plays (its own fixture id, not a built-in UUID, so the fixture never
pretends to be a Linden asset), and serve it over `ViewerAsset` like any
other asset.

Acceptance: the tokio suite fetches the animation asset and `sl-anim`
decodes it; the render-catalogue's moving-avatar check has something to
see.
