---
id: inventory-a9
title: Design Library-inventory holding & caching
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A9. Design Library-inventory holding & caching.** Source the library
    owner id + library root from the login response; hold the library tree in
    the same `Inventory` model under `owner = Library` with its own root;
    fetch it over `FetchLibDescendents2` / `LibraryAPIv3` (read-only — no
    mutation commands target it); persist it to the separate
    `<agent-uuid>.lib.inv.llsd.gz` with the same version-gated validity.
    Decide the library fetch policy (lazy on first library query vs eager
    background) given its size.

## Library-inventory reference (from A9)

The login response carries the library owner id and library root
(`inventory-lib-owner` / `inventory-lib-root`). Hold the library tree in the
same `Inventory` under the `InventoryOwner::Library` tag (A1; **not** an
`OwnerKey` variant — `OwnerKey` has only `Agent`/`Group`), root `library_root`.
Fetch it read-only over `FetchLibDescendents2` (or UDP
`FetchInventoryDescendents` with the library owner) and `LibraryAPIv3` on SL —
no mutation command targets it. Persist / load via the separate
`<agent-uuid>.lib.inv.llsd.gz` (A4) with the same version-gated validity; its
folder versions are stable, so it almost always loads fully from cache. Fetch
policy: **lazy** — a library folder's contents are fetched on first query, or in
a background slot after the agent tree, given its size.

**Verified against the code (anchors for B7).** The two login identifiers are
**already typed and surfaced** — `library_root: Option<InventoryFolderKey>` and
`library_owner: Option<AgentKey>` exist on both `sl-wire` `LoginSuccess`
(`login.rs:389`/`:393`) and `sl-proto` `LoginAccount`
(`avatar_profile.rs:350`/`:353`), copied into the held account and emitted via
`Event::Account` at `methods.rs:1202-1204` (the A1 retype, already on master).
The library **skeleton** also already arrives: `success.library_skeleton`
(`login.rs:395`, the `inventory-skel-lib` field) is folded by `skeleton_folder`
and emitted as `Event::LibraryInventory(Vec<InventoryFolder>)` (`event.rs:465`)
at `methods.rs:1213`, beside the agent `Event::InventorySkeleton`. So B7
introduces **no new login parse and no new skeleton event** — it folds the
already-emitted `LibraryInventory` folders into the model under
`InventoryOwner::Library` (seeding their `FolderState` from the skeleton
version, exactly like the agent path) and per-owner-merges the library skeleton
against the library cache (A5 `loadSkeleton(options, getLibraryOwnerID())`
precedent, `methods.rs:579-583`).

**The fetch path needs three concrete additions (B7), because today every fetch
is hardwired to the agent as owner:**

- **A new library CAPS cap constant.** `FetchLibDescendents2` does **not** exist
  in `session.rs` yet — only
  `CAP_FETCH_INVENTORY = "FetchInventoryDescendents2"` (`session.rs:63`, agent)
  and `CAP_LIBRARY_API_V3 = "LibraryAPIv3"` (`session.rs:311`, the AIS3 library
  cap) do. B7 adds `CAP_FETCH_LIBRARY = "FetchLibDescendents2"` beside
  `CAP_FETCH_INVENTORY`. Confirmed upstream: Firestorm registers
  `FetchLibDescendents2` (`llviewerregion.cpp:3489`) and POSTs library fetches
  to it (`llinventorymodelbackgroundfetch.cpp:1404`), sharing one handler with
  `FetchInventoryDescendents2` (`:141`); the AIS3 library cap is `LibraryAPIv3`
  (`llaisapi.cpp:51`).
- **No new CAPS body — just the library owner.** The body builder
  `build_fetch_inventory_request(owner_id, folder_ids)` (`llsd.rs:637`) already
  takes `owner_id`; the library fetch reuses it verbatim with
  `owner_id = library_owner`, POSTed to the `CAP_FETCH_LIBRARY` URL. But the two
  runtime dispatch arms currently pass the agent: tokio `lib.rs:512-526` and
  bevy `lib.rs:642-658` both compute `owner = session.agent_id()` and the agent
  cap URL. B7 routes a library-owned folder to `library_owner` + the library cap
  URL instead (selected by the folder's `InventoryOwner` tag).
- **Parameterize the UDP owner.** `send_fetch_inventory_descendents`
  (`circuit.rs:2779`) hardcodes `owner_id: self.agent_id.uuid()` ("Own
  inventory: the owner is the agent itself", `:2790`), reached from
  `request_folder_contents` (`methods.rs:7833`). B7 threads an `owner_id`
  through so the UDP path can fetch the library as `library_owner`.

**OpenSim testability (for A11 / live).** Stock OpenSim ships
`Cap_FetchLibDescendents = ""` (`OpenSimDefaults.ini:787`) — it does **not**
serve the library CAPS cap, so on the local grid only the **UDP** path (owner =
library owner) exercises the library fetch; the modern `FetchLibDescendents2` /
`LibraryAPIv3` paths are SL-only (the standing "SL is the target, OpenSim is the
safe test grid" constraint). The library skeleton fold, the
`InventoryOwner::Library` hold, the separate cache file, and the UDP fetch are
all OpenSim-testable; the CAPS/AIS3 library fetch is verified on SL.
