---
id: test-fake-grid-object-asset-id-divergence
title: The fake grid names an object's asset where Second Life never does
topic: test
status: done
origin: measured doing object-asset-format on aditi (2026-09-06)
points: 2
refs:
  [
    test-assets-object-asset-codec,
    test-fake-grid-asset-round-trip,
    test-object-asset-missing-fields,
  ]
---

Done 2026-09-06. The switch was built, and the default is Second Life — the
strict side. See "What landed" below.

Context: [context/testing.md](../context/testing.md).

Measured on aditi 2026-09-06 (`object-asset-format`): **Second Life gives a
viewer no asset id for an object inventory item.** Eleven of eleven object
items answered with a nil `asset_id`, in the AIS3 folder listing and again in
the per-item `GET /item/<id>`, and all eleven were full-perm to their owner —
so it is not the "no asset id unless you fully own it" rule, it is the class.
OpenSim is the opposite: every object item names an asset and `ViewerAsset`
serves it as `SceneObjectSerializer` XML.

The fake grid follows OpenSim here, because [[test-assets-object-asset-codec]]
made a take mint an id and back it with bytes. Against the grid this workspace
actually targets, that is **more permissive than the real thing**, and the
whole point of the fake grid is to fail a viewer the way a real grid would. A
viewer developed against it could come to rely on opening a taken object's
asset — something Second Life will never allow.

The decision is which grid the fake one should imitate, and it is not obvious:

- **Follow Second Life** — a taken object's item carries a nil asset id, and
  nothing can fetch the body. Faithful to the target grid, and it would have
  caught the reliance described above. It costs the `asset-round-trip` case its
  fourth leg (which asserts the take's asset *describes the object taken*, and
  is the only thing exercising `sl-object-asset`'s bridge end to end), so that
  assertion would have to move to a unit test.
- **Follow OpenSim** — keep serving it. Still a real grid's behaviour, and the
  fake grid does model OpenSim's side elsewhere.
- **Make it a scenario switch**, so a case can ask for either and a viewer test
  can pin its behaviour against both.

Whichever is chosen, the divergence should be *stated* in the fake grid's docs
rather than left as an accident of what was easy to build.

Acceptance: the fake grid's object-asset behaviour is a deliberate, documented
choice, and a viewer that assumes it can open a taken object's asset fails
against at least one configuration.

## What landed

**The third option, with Second Life as the default.**
`assets::ObjectAssetPolicy` is the switch, `FakeGridBuilder::object_assets`
sets it, and `Withheld` — Second Life — is what a grid nobody configured
behaves like. So the failing configuration is not one a test has to remember
to ask for: it is the one every existing fake-grid consumer already got.

- **`Withheld`**: a take files the item under a **nil** asset id and the
  object's body goes into a second store in `GridAssets` that no capability
  reads, keyed by the *item* id. The two stores share no keyspace, so the body
  is unfetchable by construction rather than by an unguessable id.
- **`Served`**: the OpenSim side, unchanged from what
  [[test-assets-object-asset-codec]] built — the item names a minted asset id
  and `ViewerAsset` serves the body under it.

The take reads the policy once (`world::taken_item`); `store_taken_asset` then
reads the rule back off the *item* — nil id means the withheld store — so the
two halves cannot disagree about where a body went. `rez_from_inventory` asks
the item first and the store second, which is why **the rez works under both**:
on Second Life too a taken object drags back out of inventory, because the
simulator resolves the body and the viewer never needed it. That was the one
thing this could plausibly have broken, and it is what
`a_take_withholds_the_object_asset_and_the_rez_still_works` pins.

The fourth leg of `asset-round-trip` did **not** have to move to a unit test: a
case can name the flavour it needs, and that case asks for the OpenSim one.
`object-asset-format` runs on the Second Life one, so its fake-grid leg now
records `take_step = item-created-nil-asset`, the same string it records on
aditi.

> **Superseded the same day, in the good way.** This landed as a knob of its
> own — a `GridTest::fake_object_assets` a case answered — and the review that
> followed made the right point: which grid the fake one imitates is not one
> behaviour's business. [[test-fake-grid-imitates-audit]] replaced the knob with
> `sl_fake_grid::ImitatedGrid` and split the harness's `Grid::Fake` into
> `Grid::FakeSl` and `Grid::FakeOpensim`, so a case names a *grid* rather than
> answering a question about object assets. Everything above about what the two
> sides do is still exactly what happens; only who decides changed.

**What the switch does not govern**, stated here because it is the honest
residue: the seeded `Fixture Object` (`sl-test-assets`) keeps its asset id and
stays fetchable in both configurations. It is the fake grid's own fixture,
seeded so `asset-round-trip`'s read half has an authored object body per class,
and no live grid has an item like it. Nil-ing it would have meant threading the
policy into the fixture seeding and into `sl-test-assets`' own class table for
no observable a viewer cares about — the class it *can* reach, a take's output,
is the one the switch covers. This is written down in `sl-fake-grid/README.md`
too, which is where the task asked for the divergence to be stated.
