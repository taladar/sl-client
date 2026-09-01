---
id: test-fake-grid-animation-assets
title: A synthetic animation asset the fake grid can actually serve
topic: test
status: done
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

Done (2026-09-01): `sl-test-assets/src/anim.rs` writes the modern `1.0`
keyframe-motion encoding, with `chest_twist_animation_asset()` as its one
public entry — the same shape `mesh::unit_cube_mesh_asset()` has, so all
the byte-writing machinery stays private and nothing is exported that no
fixture uses. The motion is two seconds of `mChest` twisting a sixth of a
turn about its local Z and back, looping, with **no ease in or out** so
the pose at a given time does not depend on how long the motion has been
running (a capture taken at an arbitrary moment after arrival is still
comparable). `mChest` rather than an arm or the pelvis: every skeleton has
it (no optional Bento bone), it carries the head and both arms with it so
a large patch of screen moves, and it is clear of the locomotion IK's
pelvis/leg joints.

Two round-trip tests pin it through `sl_anim::Motion::from_bytes` (a new
dev-dependency): the header fields decode verbatim, the joint track is one
joint with an empty position track and no constraints, and the three keys
land at 0 / 1 / 2 s with the middle one the twist — the last assertion
checks the quaternion dot product between the extremes is under 0.98, i.e.
the two poses really are far enough apart for a capture to tell them
apart.

`catalogue::NPC_ANIMATION` is now the catalogue's own `0xCA7_0103` instead
of the built-in `stand` UUID, and `catalogue::assets()` registers the
motion under it. Acceptance met by
`the_npc_animation_asset_is_fetchable_and_decodes` (tokio, over
`Command::FetchAsset` / `AssetType::Animation`) and by the unit test
`the_npc_animation_is_a_fixture_asset`, which also asserts the id collides
with no built-in.

One deviation, deliberate: `mesh.rs`'s `quantize` / `round_to_u16` /
`push_u16` moved up to the crate root as private helpers, because the
animation keyframes quantise over exactly the same `u16` full scale as the
mesh streams. Copying ten lines to write a second `u16`-quantised wire
format would have been the third copy waiting to happen.

Live-verified against the standalone grid (`--catalogue`) with the real
viewer: the log shows `resolving animation …0ca70103 ('uploaded')` →
`decoded (1 joint track(s))` → `posing avatar …0ca70100 skeleton`, so the
fixture motion is fetched over `ViewerAsset` and actually drives the NPC's
bones. The built-in path is unchanged and still 404s — filed as
[[test-fake-grid-builtin-animation-assets]] (with the six built-in UI
sounds it turned out to be sitting beside).
