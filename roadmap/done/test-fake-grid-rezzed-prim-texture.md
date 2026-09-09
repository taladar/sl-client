---
id: test-fake-grid-rezzed-prim-texture
title: A prim the fake grid rezzes carries no TextureEntry
topic: test
status: done
origin: noticed doing test-assets-object-asset-codec (2026-09-06)
points: 1
refs: [test-fake-grid-object-write-path, test-assets-object-asset-codec]
---

Done (2026-09-09). A prim this grid makes now wears the default plywood
(`sl_proto::DEFAULT_PRIM_TEXTURE`) on every face, which is what a real
simulator puts on a prim it rezzes.

**One entry, no face count.** `world::default_texture_entry` packs a single
plywood face and both prim constructors — `box_prim` and the rez path's
`prim_from_shape` — assign it. It deliberately takes no face count: the entry
is run-length packed, so an entry whose faces all agree is one default and no
overrides, and the bytes are the same however many faces the shape renders. A
client that decodes the blob for a plain box's six and one that decodes it for
a hollow cut box's nine both read plywood everywhere, which is asserted from
*one* object in `a_rezzed_prim_wears_the_default_texture`.

`bare_object` stays the neutral skeleton and `avatar_prim` still carries no
entry at all, on purpose: an avatar's faces are its bake slots and what fills
them travels in the `AvatarAppearance` the grid pushes separately, so the prim
default there would be plywood *in a bake slot*.

**Both questions the task left open, answered.**

Nothing renders differently by assertion. No baseline file and no baseline test
renders a fake-grid object — the twelve under `baselines/` are fed by
`render_scene.rs`, which builds its geometry synthetically and records LOD
counts, bounds and framing rather than colour. The only tier that renders the
fake grid is `full_stack_test.rs`, and none of its oracles reads the stock
scripted box: its parcel-line framing is explicitly chosen *clear* of it, and
every colour oracle reads catalogue or border fixtures, which are
`PrimFixture`-built and already plywood or checker. It does render differently
by eye — the stock box was opaque white (`objects.rs` falls back to a nil
texture face for an object with no entry, not to plywood) and is now
plywood-brown, which is what it should have looked like all along. The library
already serves the id (`scenario::default_assets`, pinned), so the settle loop
has nothing new to wait for; the 21 full-stack tests pass unchanged.

The task's parenthesis about the **`object-edit` case is wrong**: that case
sets no texture. Its twelve edits are name, description, permissions, for-sale,
category, material, flags, shape, click-action, include-in-search, transform
and undo/redo, and no conformance case anywhere sends `SetObjectImage`. The
fake grid's `ObjectImageSet` arm is exercised only by the build floater's
Texture tab test and `sl-proto`'s `sim_session` round-trip, and both *replace*
the entry wholesale, so neither depended on the prior state being empty.

**Something else did read an empty entry, in the tier below.** Not the fake
grid: `world_test::fixture_prim`, the tier-I fixture prim, sent no entry
either, and the build floater's Texture tab test had already worked around it
with a private `seed_textured_prim` whose comment argued the general case ("a
real prim always carries an entry, so the fixture carries one too"). The entry
moved into `fixture_prim` and the workaround is gone. Those faces name the
**nil** texture rather than plywood, so that no tier-I fixture asks a world
with no asset source for an asset it cannot serve; nil renders untextured,
which is exactly what those fixtures looked like before they had an entry at
all. Two viewer-side branches change behaviour for the better as a result:
`objects.rs` now marks a rezzed prim's first update as retextured, and caches
the entry the Texture tab reads.

The take's `taken_prim` pad survives, narrowed to what still reaches it — an
object rezzed from an asset that stated no faces — because an object asset with
no faces rezzes an object nothing can texture either way.

Verified by `a_rezzed_prim_wears_the_default_texture` and
`a_taken_rezzed_prim_states_its_default_faces` (new), the 204 `sl-fake-grid`
tests, the 112 `sl-conformance` offline cases (`object_rez_derez`,
`object_edit`, `object_asset_format` among them), the 87 world / build-floater
tier-I tests and the 21 full-stack renders — all green.

---

Context: [context/testing.md](../context/testing.md).

`world::bare_object` leaves `texture_entry` empty and nothing on the rez path
(`ServerEvent::RezObject` → `prim_from_shape`) fills it in, so a prim a client
rezzes against the fake grid arrives with **no per-face data at all**. A real
simulator gives a new prim the default texture — the plywood
`89556747-24cb-43ed-920b-47caed15465f`, which is the very id the reference's
own object asset carries on all six faces.

It surfaced through the take: `store_taken_asset` has to state the faces the
shape renders, and `decode_texture_entry` of an empty blob is *no faces*, so
the take writes untextured faces and says so in a comment rather than letting
the asset claim the prim has none.

Two things to settle:

- whether a rezzed prim should get the default texture here (it should, if
  nothing renders differently for it — check the render baselines and the
  `object-edit` case, which sets a texture on a prim it rezzes);
- whether anything else reads a rezzed prim's texture entry and quietly gets an
  empty one.
