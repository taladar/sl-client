---
id: test-assets-object-asset-codec
title: Read and write an inventory object asset
topic: test
status: ready
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 5
refs: [test-shared-test-assets, viewer-task-inventory-open-and-save-back]
---

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
