---
id: test-assets-object-asset-codec
title: Read and write an inventory object asset
topic: test
status: done
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 5
refs:
  [
    test-shared-test-assets,
    viewer-task-inventory-open-and-save-back,
    test-fake-grid-asset-round-trip,
    test-fake-grid-object-write-path,
    test-assets-remaining-class-audit,
  ]
---

Done 2026-09-06. `sl-object-asset` is the crate; a take now authors the asset
its item names; see "What landed" below.

Context: [context/testing.md](../context/testing.md).

`AssetType::Object` — what an inventory object item points at, the
serialised prim or linkset a rez restores — has no codec. The class
appears only as an inventory-item type and a group-notice icon. Every
object a fixture shows today is built as a live `sl_proto::Object` and
pushed over `ObjectUpdate` (`PrimFixture`), which is the *wire* form, not
the asset form; the two are unrelated encodings.

So nothing can test the paths that go through the asset: rezzing an
object from inventory, taking one back, a coalesced object, an object
offered in an IM, or the object embedded in a notecard
(`sl-proto/src/types/editing.rs:713` already names that case).

The format is the reference's legacy `LLSD/Binary`-prefixed or plain
newtype text of `LLViewerObject`'s inventory serialisation — the
`{'task_id': …} { name Object| permissions {…} shape {path {…} profile
{…}} faces N {imageid …} … }` nested-block text, one block per prim in a
linkset. Firestorm's `indra/llcommon/tests/commonmisc_test.cpp:437`
carries a complete single-prim example, which is a ready-made fixture to
pin a parser against.

Wanted:

- a decoder into a typed object-asset model (permissions, sale info,
  shape, per-face texture entry, name-values, the child prims of a
  linkset);
- an encoder, so `sl-test-assets` can write a one-prim object and a
  two-prim linkset;
- a bridge to `sl_proto::Object` in at least one direction, so a fixture
  can rez what it serialised and assert the two agree.

Sized at 5 rather than 3 because the block grammar is deep and the
per-face section overlaps the `TextureEntry` encoding that already exists
— the decoder should reuse it rather than re-parse.

Acceptance: the reference's example prim parses; a written object round
trips; a fake-grid fixture can serve an object asset by id.

## What landed

**`sl-object-asset`**, a format crate beside `sl-notecard` and `sl-prim`:
`ObjectAsset::{decode, decode_str, encode, encode_to_string}` over a typed
model (`PrimBlock` and its `LegacyPermissions` / `LegacySaleInfo` /
`LegacyShape` / `LegacyFace` blocks), plus a `bridge` both ways between a prim
block and an `sl_proto::Object`.

**Where the grammar comes from, since the viewer's own reader is gone.** Three
sources, and the crate docs say which part rests on which: the sub-block
writers that *do* survive (`LLPermissions` / `LLSaleInfo` / `LLPathParams` /
`LLProfileParams::exportLegacyStream`, and their `importLegacyStream` twins for
the reading rules); the two complete assets the reference carries in its own
test sources — the single attachment prim in `commonmisc_test.cpp` the task
named, **and a four-prim linkset in `lluri_test.cpp`** the task did not know
about; and `LLTextureEntry`'s LLSD field names, which are the `faces` block's
own names.

The linkset is what made the child half knowable: a child prim writes
`childpos` / `childrot` where a root writes `velocity` / `angvel`, and a
`linked child` / `linked linked` line marks the role — the root is written
**last**. Both assets are in `tests/data` (one string redacted, see the README
there) and `tests/reference.rs` pins every field of the first and the linkset
shape of the second.

**Three decisions worth recording:**

- **An unknown keyword is kept, not dropped.** The reference warns and moves on,
  which makes its reader a lossy editor for anything a newer simulator writes.
  `PrimBlock::unknown` carries them and the encoder writes them back.
- **A malformed value is an error.** The reference's `sscanf` leaves the field
  at whatever it held, so a bad `pos` line silently leaves a prim at the origin.
- **The round trip is semantic, not byte-level.** The reference writes each
  number at whatever precision its own `ostream` had — six significant digits
  for a position, a float's full exact decimal expansion for a rotation, within
  one prim — and this crate writes the shortest text that round-trips. A
  reference asset decodes, re-encodes and re-decodes to the same model.

**`PrimShape::to_params`** is new in `sl-prim`: the quantizing inverse of
`from_params` (`LLVolumeMessage::packPathParams` / `packProfileParams`), which
is what lets an asset's float shape go back on the wire. It belongs there,
beside the dequantizer, not in a second copy here.

**The take is backed now.** `sl-fake-grid`'s `store_taken_asset` serialises the
live object and stores it under the id `taken_item` mints, closing the half
[[test-fake-grid-asset-round-trip]] left open. `sl-conformance`'s
`asset-round-trip` grew a fourth leg for it: rez a cube, take it, fetch the id
the item names, and assert the asset *describes the object taken* — the prim
key, the name the item carries, the scale, the face count, and the shape block
re-quantizing to the object's own `PrimShapeParams`. That last one is the real
assertion: a take is the one asset nobody uploaded, so "the id resolves" alone
would pass for any bytes at all.

**The fixture table** has a `Fixture Object` entry (a one-prim body, a two-prim
linkset as the second), so `AssetType::Object` leaves
`sl_test_assets::inventory::unsupported_classes` — the recorded "no" this task
was the answer to.

## What the format cannot carry

Worth knowing before anyone treats a take as lossless: the asset text has no
field for a face's `glow` or legacy material id, none for `ExtraParams` (flexi,
light, sculpt, mesh, reflection probe), none for floating text (only its
*colour* is written), none for a media URL, and none for a texture animation or
particle system. A prim carrying any of those does not survive the trip. That
is the format, not the crate — inventing keywords would produce an asset no
grid could read — and it is filed as [[test-object-asset-missing-fields]].
