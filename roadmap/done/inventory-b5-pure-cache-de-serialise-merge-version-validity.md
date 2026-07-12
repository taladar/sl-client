---
id: inventory-b5
title: Pure cache (de)serialise + merge/version-validity
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B5. Pure cache (de)serialise + merge/version-validity (from A3·A5)

- [x] Add `sl-proto/src/session/inventory_cache.rs` (pure — no I/O, gzip, or
      clock): an `inventory_to_cache_bytes(&Inventory, owner)` builder returning
      `Result<Vec<u8>, WireError>` (4-byte BE version header + binary LLSD via
      B2 + the existing `_to_llsd` converters over the `Loaded`-only
      [`Inventory::cacheable_snapshot`]), and an
      `inventory_from_cache_bytes(&[u8]) -> Result<Option<CachedInventory>,
      LlsdError>` parser (`None` ⇒ cold: short/wrong-version header; `Err` ⇒ a
      version-`5` payload that fails to decode), plus a `load_cached_into(&mut
      Inventory, &CachedInventory, owner)` fold that lands each cached folder
      `Loaded` at its stored version.
- [x] Add the merge method `Inventory::merge_skeleton(&mut self, skeleton:
      &[InventoryFolder], owner) -> Vec<_>`, run **once per
      owner** (agent skeleton `methods.rs:1216-1226` vs library skeleton
      `:1207-1213` / `Event::LibraryInventory`): version-match keeps the cached
      contents + `Loaded`; mismatch / skeleton-only ⇒ `Unknown` (dropping its
      cached children); a cached folder of that owner absent from the skeleton
      is dropped; items kept only under a folder that stayed `Loaded`. Return
      the `Unknown` set as the B6 fetch queue. (Firestorm `loadSkeleton`
      `llinventorymodel.cpp:3025-3171`, `VERSION_UNKNOWN = -1` ⇒ `Unknown`.)
- [x] Tests: bytes round-trip to an equal model; version-header mismatch ⇒
      rejected (and merge against that empty result ⇒ every skeleton folder
      `Unknown`); merge keeps a matching folder (absent from the fetch set) and
      drops a stale one (present in the fetch set); a server-deleted folder is
      dropped; an item under a now-`Unknown` folder is dropped while one under a
      kept `Loaded` folder survives.

**Landed (notes for later tasks).** Three refinements from the drafted shape,
all internal (no scope change): (1) the module is a **session submodule**
(`sl-proto/src/session/inventory_cache.rs`, not top-level) so it reaches the
`pub(crate)` `_to_llsd` converters and the held [`Inventory`] directly; (2) the
held model is **one** `Inventory` holding both trees, so `merge_skeleton` is an
in-place `&mut self` method taking an `owner` (rather than consuming/returning a
standalone `Inventory`) and `load_cached_into` folds into the existing model —
the per-owner filter is what keeps the other tree intact; (3) the pure functions
are surfaced for B10 (and kept non-dead) by three `pub` `Session` wrappers —
`inventory_cache_bytes(owner)`, `load_inventory_cache(owner, &[u8]) -> bool`
(false ⇒ cold), and `merge_inventory_skeleton(owner, skeleton)`. The cache
version `5` is exported as the crate-root `INVENTORY_CACHE_VERSION`, and
`parse_llsd_binary` is now re-exported from `sl-wire`. Login-flow wiring (load
before skeleton, merge on skeleton, save on logout) stays **B10**.
