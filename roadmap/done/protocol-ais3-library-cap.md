---
id: protocol-ais3-library-cap
title: Read both inventory trees over AIS3 on Second Life, the library over LibraryAPIv3
topic: protocol
status: done
origin: measured doing object-asset-format on aditi (2026-09-06)
points: 2
refs: [protocol-ais3-nested-embedded]
---

## Done (2026-10-04)

Both trees are read over AIS3 wherever it is granted, in both runtimes:

- `sl-proto`: an AIS3 *fetch* reply has its own routing tags
  (`AIS3_FETCH_INVENTORY_TAG` / `AIS3_FETCH_LIBRARY_TAG`) and decoder
  (`ais_folder_listing_from_llsd`): one `InventoryDescendents` per folder the
  listing opened, folded into the right tree and marked loaded at its version,
  the descendent count being what the listing names (AIS states none).
- `sl-client-bevy` / `sl-client-tokio`: the background crawl and single-folder
  loads take `GET <cap>/category/<id>/children?depth=0` on `InventoryAPIv3` /
  `LibraryAPIv3` by folder owner, then the descendents caps, then UDP;
  `Ais3FetchFolderChildren` routes a Library folder to `LibraryAPIv3`; the
  descendents fetch logs every failure instead of returning silently.
- Automation: an `inventory_tree` probe (every folder of a tree by id) and
  `sl-viewer-ctl inventory [--library] [--wait-loaded SECS]`.

**Verified on aditi** by dumping both whole trees from a headless viewer after a
search query swept every folder (the viewer's model holds a folder's items once
the UI has paged it): AIS3 against the descendents caps, same build — Library
490 folders / 5581 items, **zero differences** (Kart 1.0 in `Library/Objects`);
agent 36 folders, identical but for two Lost And Found objects auto-returned
between the runs. `library-tree-fetch` and `inventory-tree-crawl` pass on aditi
(tokio runtime, AIS3) and on OpenSim (fallback). Found on the way:
[[viewer-inventory-settings-subtree-shows-unloaded]].

Second Life serves the shared **Library** inventory over its own capability,
`LibraryAPIv3`, alongside the agent's own `InventoryAPIv3`. This workspace
handles both on the *reply* side — `sim_caps.rs` maps them to the same
`CapHandler::Ais3` and `methods.rs` tags the resulting folders and items with
`InventoryOwner::Library` — but every AIS3 *request* command
(`Ais3FetchFolderChildren` and its siblings, `sl-client-tokio/src/lib.rs`)
looks up `CAP_INVENTORY_API_V3` only.

So a library folder id sent to the inventory capability is a 404, observed on
aditi 2026-09-06:

```text
WARN sl_client_tokio::http: a CAPS GET was rejected
     capability="InventoryAPIv3" status=404 Not Found
```

The fix is to route by the folder's owner: a request for a folder known to be
the library's — or descended from the library root the login response names —
goes to `CAP_LIBRARY_API_V3`. The command carries only a folder id today, so
either the id is resolved against the cached tree before the request, or the
command grows an owner field.

Worth doing because the library is the one part of an account's inventory that
is **full-permission by construction**, which makes it the control group for
any question of the form "does the grid treat this item differently because of
its permissions" — `object-asset-format` wanted exactly that and could not
have it.

Acceptance: a library folder's children fetch on Second Life, and the items
arrive tagged as the library's.

## Widened (2026-10-03)

The viewer itself loads the library — and the agent's own tree — over the
older `FetchLibDescendents2` / `FetchInventoryDescendents2` caps (the
background crawl and single-folder loads in both runtimes), so the routing
bug above never shows in it. Firestorm reads both trees over AIS3 whenever
AIS is available (`LLInventoryModelBackgroundFetch::bulkFetchViaAis`:
`GET <cap>/category/<id>/children?depth=0`, `LibraryAPIv3` for folders the
library owner holds) and keeps the descendents caps as its fallback. So:

- a session route for an AIS3 *fetch* reply (distinct from a mutation reply)
  that folds the children into the right tree, marks the folder loaded at its
  version and emits `InventoryDescendents`, as the descendents caps do;
- both runtimes read over `InventoryAPIv3` / `LibraryAPIv3` by folder owner
  where granted (crawl, single folder, `Ais3FetchFolderChildren`), falling
  back to the descendents caps, then UDP;
- fetch failures logged and surfaced instead of returning silently;
- verified on aditi by dumping the whole agent and library trees through the
  automation before and after the switch and diffing them.
