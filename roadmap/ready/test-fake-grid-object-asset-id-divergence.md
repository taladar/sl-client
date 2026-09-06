---
id: test-fake-grid-object-asset-id-divergence
title: The fake grid names an object's asset where Second Life never does
topic: test
status: ready
origin: measured doing object-asset-format on aditi (2026-09-06)
points: 2
refs:
  [
    test-assets-object-asset-codec,
    test-fake-grid-asset-round-trip,
    test-object-asset-missing-fields,
  ]
---

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
