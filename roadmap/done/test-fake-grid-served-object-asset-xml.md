---
id: test-fake-grid-served-object-asset-xml
title: The OpenSim-flavoured grid serves an object body OpenSim never wrote
topic: test
status: done
origin: the leftover of test-object-asset-missing-fields (2026-09-09)
points: 5
refs:
  [
    test-object-asset-missing-fields,
    test-fake-grid-object-asset-id-divergence,
    test-assets-object-asset-codec,
  ]
---

Done (2026-09-09). Both directions, and the decision the task said to make
first.

**Written *and* read.** The task offered write-only as enough to make `Served`
honest. It is not enough for long: the same round trip that proves the writer
is the reader, the rez path now dispatches on the body's own bytes rather than
on the policy, and anything that fetches a taken object's asset off a live
OpenSim — the very cross-check this task exists to unblock — needs the reader
and not the writer.

**Where it lives:** `sl-object-asset/src/opensim/`, one module per job
(`model`, `encode`, `decode`, `bridge`) mirroring the crate's own split, and
the crate is now explicitly *two formats, one per grid* rather than "the Second
Life one". A separate crate would have bought only boilerplate: the class is
one asset type and this is the other half of it.

**What the fake grid does with it.** `store_taken_asset` writes the Linden text
for a withheld body and the XML for a served one, reading the rule off the item
exactly as before. The take now also gathers each prim's **task inventory** out
of the region and writes it into the served body — OpenSim's
`WriteTaskInventory` — because an object update carries no contents and a body
that stated none would file a scripted prim as an empty one. `rez_asset_body`
dispatches on the first non-whitespace byte (`<` is XML, `{` is the text), so a
grid can rez a body written under the other flavour instead of failing on it.

**Two conversions turned out not to be the identity**, and both are now tests
rather than assumptions:

- hover-text alpha is **inverted** between the XML and the wire. OpenSim stores
  `Color.A` as opacity and sends `0xFF - A` (`SceneObjectPart.GetTextColor`);
  the reference viewer inverts it straight back
  (`llviewerobject.cpp`). A bridge that copied it through would file every
  visible label as an invisible one.
- a light's **alpha byte on the wire is its intensity**
  (`PrimitiveBaseShape.ExtraParamsToBytes` says so in a comment), so the XML's
  `LightColorA` is a different value that never crosses the wire and stays at
  the 1.0 OpenSim constructs it with.

**Prim-flag names came out of the DLL, not out of memory.** OpenSim writes the
flags as the *names* of the set bits (`WriteFlags` strips C#'s commas,
`Util.ReadEnum` puts them back), so the table has to be exact — and the
`OpenMetaverseTypes.dll` OpenSim ships has `ObjectTransfer` at `0x0002_0000`
where upstream libopenmetaverse has `0x0004_0000`. A table written from memory
would have misnamed every flag above `AllowInventoryDrop`.

**Not modelled, and not lost:** a vehicle's parameters, a physics-inertia
block, `DynAttrs`, `SOPAnims` and a keyframe motion are each another
serialiser's format and nothing here produces one. An element the module has no
field for is captured verbatim and written back, the way
`PrimBlock::unknown` keeps an unknown keyword of the text — and OpenSim's
reader is order-independent (a name → handler dictionary), so re-emitting them
at the end of the part reads identically.

**One case had to change, and was wrong before.** `asset-round-trip`'s fourth
leg decoded a taken object's body as the Linden text while declaring
`Grid::FakeOpensim` — which would have failed against a real OpenSim for a
reason unrelated to what it tests. It now decodes `<SceneObjectGroup>` and
asserts the packed `TextureEntry` blob rather than a face count, which is the
stronger claim this format actually makes.

Verified by the new `sl-object-asset` tests (the mirror-image
`the_xml_carries_the_whole_modern_prim`, the round trips, the flag table, the
unmodelled-element capture), the rewritten fake-grid unit test that asserts
*both* flavours' bodies side by side, and the two end-to-end takes.

---

Context: [context/testing.md](../context/testing.md).

`ObjectAssetPolicy::Served` exists to be **OpenSim's** side of the object-asset
divergence: the item names a minted asset id and the grid serves the body under
it, which is the one configuration where a taken object's bytes cross the wire.
The bytes it serves are the Linden **text** form, and OpenSim has never written
that format for anything. It writes
`SceneObjectSerializer.ToOriginalXmlFormat` — or
`CoalescedSceneObjectsSerializer.ToXml` for a multi-object take — as the
`AssetType.Object` body (`InventoryAccessModule.cs:527-587`).

So a viewer that fetched a taken object's asset from a fake grid asked to be
OpenSim would get bytes no OpenSim would ever hand it. Everything else about
that policy is faithful — the id is minted, named in the item and served — and
this is the last thing about it that is not.

Nothing forces the work, which is why it is a task and not a bug: **no viewer
in this workspace has a reader for either format**, and Second Life exposes no
object asset at all, so nothing downstream can currently tell the difference.
It matters the day something does — a conformance case that cross-checks a take
against a live OpenSim, or a viewer that grows an object-asset reader.

Wanted: `<SceneObjectGroup>` XML, written and read. It is a format crate's
worth of work (`sl-object-asset` is 2900 lines for the simpler of the two), and
nothing in the workspace parses or writes a single element of it today — the
name appears only in prose. The shape it would take:

- a sibling module or crate for the XML, with `sl-object-asset`'s own split
  between the model, the codec and the `sl_proto::Object` bridge;
- `Served` writing that body instead of the text, and reading it back;
- the text form staying exactly what it is, for `Withheld` and for the two
  reference captures — the two grids genuinely disagree about what
  `AssetType::Object` *is*, and the fake grid should disagree with itself in
  the same place.

Worth deciding first, and cheaply: whether OpenSim's XML should be **read** as
well as written. The rez path no longer needs it — since
[[test-object-asset-missing-fields]] the grid rezzes from the linkset a take
removed rather than from any body — so a write-only serialiser would already
make `Served` honest, and a reader is only wanted if something is going to hand
the grid an OAR-flavoured body it did not write.

Acceptance: a grid started with `ObjectAssetPolicy::Served` serves a taken
object's asset as `<SceneObjectGroup>` XML that OpenSim's own deserialiser
would accept, with the prim's light, flexi, sculpt/mesh, glow, text, media,
texture animation and particle system in it — the fields
[[test-object-asset-missing-fields]] measured the text losing.
