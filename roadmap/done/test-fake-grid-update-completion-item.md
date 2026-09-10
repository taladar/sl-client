---
id: test-fake-grid-update-completion-item
title: A fake grid that echoed a field the real one withholds
topic: test
status: done
origin: Live aditi run of the settings editor's Save (2026-09-10)
refs: [viewer-settings-save-as-create-then-put]
---

Context: [context/test.md](../context/test.md).

An **update** capability's completion is only obliged to carry the new *asset*:
the item already exists and the client is the one that named it. The two real
grids therefore disagree about whether it also names the item, and the fake grid
modelled only the lenient one.

- **OpenSim echoes it.** `UpdateItemAsset.cs` answers
  `uploadComplete.new_inventory_item = m_inventoryItemID`.
- **Second Life does not**, and the reference client never asks it to:
  `LLBufferedAssetUploadInfo::finishUpload` — the update path — reads
  `result["new_asset"]` and takes the item from `getItemId()`, the id it sent in
  the request. Only the *create* path (`LLResourceUploadInfo::finishUpload`)
  reads `new_inventory_item`.

## Why it mattered

A grid more forgiving than the real one is a test target that certifies bugs.
The viewer's settings editor correlated "my save landed" on the echoed item; it
passed every offline test against the fake grid and then reported "Saving…"
forever against aditi on a save that had in fact succeeded, never clearing its
unsaved-changes flag — which is how it came to prompt about losing changes that
were already stored. See [[viewer-settings-save-as-create-then-put]].

## Done

`SimSession::set_update_completion_names_item` decides it, defaulting to Second
Life's answer, and `ImitatedGrid::update_completion_item` gives the two flavours
their own defaults — a new row in the divergence table, with
`FakeGridBuilder::update_completion_item` to override it per test as every other
divergence can be. `UpdateCompletionItem::Omitted` is the stock answer because
it is the **stricter** of the two: a client that works against a grid which
never echoes works against both, and one that depends on the echo now fails
offline rather than in front of a person.

The **driver** is still told the item either way — an update has to be applied
to the item it replaces, and once the metadata is consumed the event is the only
place that id survives. The `sim_caps` test pins both halves, because a reply
that started naming the item and a driver event that stopped would be equally
wrong.

## A second divergence this turned up

The fake-grid branch had measured that Second Life sends
`UpdateCreateInventoryItem` **again** when a capability upload rewrites an item
(its `UploadAnnouncements::saved` row). On that grid the message meaning "here
is the item you asked me to make" and the one meaning "the item you just saved
has a new asset" are the same message, and the viewer's settings-creation queue
popped on both — so a Save's announcement could spend the entry belonging to a
creation still in flight, writing that creation's body onto the item somebody
merely saved.

Guarded definitionally rather than heuristically: a creation is one where the
item did not exist before, so an item the inventory mirror already holds is a
rewrite and spends nothing. Inert against OpenSim, which announces nothing after
a rewrite.

## Verified

`cargo test --release -p sl-fake-grid -p sl-proto -p sl-viewer-inventory` — 1298
green, including a new flavour test (the two answers differ, and the stock grid
is the strict one) and the updated `sim_caps` round trip. `cargo clippy` clean
on all three.
