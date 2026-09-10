---
id: viewer-uploaded-item-never-enters-the-model
title: An item the viewer just uploaded is not in its own inventory
topic: viewer
status: done
origin: found measuring the grids for test-fake-grid-imitates-upload-announcements (2026-09-08)
points: 2
refs:
  [
    test-fake-grid-imitates-upload-announcements,
    test-fake-grid-imitates-sl-new-file-upload-announcement,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-inventory` learns about a newly uploaded item only if the grid
**pushes** it. `ingest_inventory`'s `SlSessionEvent::AssetUploaded` arm calls
`rebind_saved_asset`, and that resolves through `InventoryModel::rebind_asset`,
which walks the folders it already holds and returns `None` for an item it does
not know — so a `NewFileAgentInventory` upload, whose whole point is that the
item is new, changes nothing in the model. The item is in the tree on the grid
and absent from the window until something re-fetches its folder.

Invisible until now because the fake grid announced every upload with the
legacy UDP message, and against a grid that announces, the item arrives through
the `InventoryItemCreated` path instead. **No live grid announces this one.**
OpenSim was measured (2026-09-08) sending nothing after either capability
upload, and the aditi run of `asset-upload` later the same day found Second Life
silent after a `NewFileAgentInventory` completion too — the push it does send
follows an in-place *save*, where the item already exists and the model already
holds it. So this is not a divergence a viewer can be right about on one grid:
it is every grid, and `sl_fake_grid::UploadAnnouncements::created` is `Silent`
on both flavours, which makes the gap reachable offline whichever one a test
picks.

The reference viewer does not have the problem and shows the fix:
`LLBufferedAssetUploadInfo::finishUpload` builds an `LLViewerInventoryItem` out
of the capability's own response body — the folder, permissions, asset id and
item id are all in it — hands it to `gInventory.updateItem` and calls
`notifyObservers()`. It never waits for a push, which is exactly why a grid
that sends none is fine for it.

- Build the item from `Event::AssetUploaded` (and `ScriptUploaded`) where
  `new_inventory_item` names an item the model does not hold, rather than
  returning early: the upload command knows the folder, name, description and
  permission masks it asked for, and the completion names the ids.
- Keep the announcement path working: after an in-place save a
  Second-Life-flavoured grid pushes the rewritten item (measured), which the
  model already holds and must not gain a second row for.
- Assert it offline against either fake flavour — both are silent after a
  creation — as an upload followed by "the item is in the model" with no folder
  re-fetch in between.

Acceptance: an upload against a silent grid puts the new item in the inventory
model without a folder re-fetch, and an upload against an announcing grid still
leaves exactly one row.

## Done (2026-09-10)

The item is assembled where the reference assembles it — out of the request and
the completion — and filed by the runtime, not by the window:

- `sl_proto::uploaded_inventory_item(request, response, owner, creation_date)`
  is `LLResourceUploadInfo::finishUpload` in Rust, and `Event::AssetUploaded`
  gained a `created` field carrying what it built (`None` for a baked texture
  and for every `Update*Inventory` save, which rebind an item that exists).
  `ScriptUploaded` does **not** grow one: a script upload always names an
  existing item, so there is nothing to build from.
- Both runtimes now keep the `NewFileAgentInventoryRequest` past the POST and
  call `Session::cache_uploaded_item` on the completion, *before* the event goes
  out — so the session's model, which is what a folder page is resolved from, is
  the single place the item lands. A viewer-only insert would have been wiped by
  the next page query.
- The window's part is the one the announcement path already had:
  `push_recent` + re-read the folder. An announcing grid's push dedups against
  it (`push_recent` is keyed by item id, and the tree rows come from the page),
  so the row count is unchanged there.

Two things the plan above did not know:

- **The grid says what it granted.** `new_next_owner_mask` / `new_group_mask` /
  `new_everyone_mask` (and `inventory_flags`) are now parsed, and they — not the
  masks the request asked for — are the item's. The reference is explicit about
  why ("do not assume we got the perms we asked for"). A completion carrying
  none of them falls back to the reference's assumption, which is *not* the
  request either. OpenSim spells the flags field `inventory_item_flags`, which
  no stock viewer reads; both spellings are accepted.
- The fake grid now reports its grant on a creation, and its own created item
  gained OpenSim's base/owner masks (`PermissionMask.All`, not the next-owner
  mask), so the item it holds and the item a client assembles from its
  completion agree on every field the completion reports.

Verified offline: `sl-fake-grid`'s
`no_flavour_announces_an_item_a_capability_upload_created` now also pages the
folder out of the held model and finds the item, against both imitated flavours,
with no re-fetch in between; plus unit tests for the assembly (`sl-proto`), the
completion fields (`sl-wire`), the `fair_use_fixed` mask rules and the window's
two arms (`sl-viewer-inventory`).
