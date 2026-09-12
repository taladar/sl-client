---
id: viewer-in-place-save-reverts-to-old-asset
title: A saved notecard read back blank, the session never hearing the save
topic: viewer
status: done
origin: reported live on the local OpenSim grid while verifying the notecard
  preview fix (2026-09-12)
refs: [viewer-notecard-editor, viewer-notecard-preview-ignores-unsaved-text,
  viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

Save a notecard on the local grid, close it, open it again: **empty**. The grid
was not at fault — its DB held the saved 771-byte asset and the item pointed at
it. The viewer had simply gone back to naming the asset the save replaced.

## What it was

A regression, and a three-layer one.

1. The **session** (`sl-proto`) was never told about a save. It is told about an
   upload that *creates* an item (`cache_uploaded_item`, added because no grid
   announces one), but an upload that *rewrites* one left its held item naming
   the old asset for the rest of the login. OpenSim announces nothing at all
   here — `InventoryAccessModule.CapsUpdateInventoryItemAsset` stores the asset,
   rebinds the item server-side and returns, with the client notify commented
   out — so the completion is the only word there is.
2. `QueryInventoryFolder` is a **local page of the held model**, not a grid
   re-fetch. So `rebind_saved_asset`'s "refresh the folder the item lives in"
   (`sl-viewer-inventory/src/inventory.rs`) answered out of the stale session,
   and **overwrote** the viewer's own correct rebind a frame or two later.
3. Re-opening then fetched the pre-save asset, which on OpenSim is a fresh
   notecard's one-NUL-byte placeholder — read as an *empty notecard*, exactly
   as `decode_notecard_asset` is meant to. Hence blank rather than an error.

Why it used to work: before `f50d7740` (2026-09-10) OpenSim's completion reached
the viewer with no item id, so the ingest arm that re-queries the folder never
matched and the editor's own rebind survived. That commit made the runtime fill
the item in when the grid omits it — correct in itself, and it switched on the
stale re-query behind it.

The fake grid had already written this down:
`only_a_second_life_flavoured_grid_announces_an_item_an_upload_rewrote` says a
client that reads the completion instead of waiting for a push "keeps an item
naming the asset the save replaced, which is the trade this row makes visible".

## Fix

The session learns what a save did, the way it already learns what a create did:

- `Session::rebind_saved_item_asset` points a held item at the asset a save
  wrote, and `saved_item_rebinding` is the one place that decides which
  completions are an in-place save (an `AssetUploaded` naming an item and
  creating none; a `ScriptUploaded` that stored an asset) and of what.
- Both runtimes call it **before the completion event goes out**, so anything
  that re-reads the folder on that event sees the new asset. In
  `sl-client-tokio` the save uploads now complete through the same
  session-first channel the created-item upload already used.
- And the item is named by whoever knows it: `named_item` fills the
  completion's item in from the id the client sent when the grid omits it
  (Second Life's update path never echoes it; the reference reads
  `getItemId()`). The `sl-client-bevy` asset update already did this; the tokio
  runtime did not, and neither runtime did it for a **script** save.

## Tests

- `sl-proto`: the held item rebinds, only the asset moves, the folder page hands
  the new asset back, a no-op rebind is not a cache write, and an unheld item is
  refused rather than invented. Plus the classification: a baked texture (no
  item) and a refused compile (no asset) rebind nothing.
- `sl-fake-grid` end-to-end, **both flavours**, because they withhold different
  halves: OpenSim echoes the item and announces nothing, Second Life announces
  and omits the item. The OpenSim leg fails without the rebind; the Second Life
  leg fails without taking the item from the id the client sent. Asserted
  through the folder page — what the viewer actually reads — not the asset
  store.

Confirmed as a true A/B: the end-to-end test fails on the code as it was.

Live-verified on the local OpenSim grid: a notecard saved, closed and reopened
comes back with its text, over repeated saves in one session.
