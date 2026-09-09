---
id: test-fake-grid-served-object-asset-xml
title: The OpenSim-flavoured grid serves an object body OpenSim never wrote
topic: test
status: ready
origin: the leftover of test-object-asset-missing-fields (2026-09-09)
points: 5
refs:
  [
    test-object-asset-missing-fields,
    test-fake-grid-object-asset-id-divergence,
    test-assets-object-asset-codec,
  ]
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
