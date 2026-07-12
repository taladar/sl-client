---
id: inventory-b12
title: Update the mdbook inventory chapter (docs)
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B12. Update the mdbook inventory chapter (docs) — DONE

The `book/src/content/inventory.md` chapter still describes only the old
fetch / mutate / rez / server-push flow; the whole held read-model added across
B3–B8 is undocumented. Bring the chapter in sync with the new public API.

- [x] Document the held two-level model (B3): the in-memory `Inventory` holding
      the agent tree **and** the read-only Library tree, the per-folder
      `FolderState` (`Unknown` / `Fetching` / `Loaded { version }`), the
      parent→children index, and that it holds structure/metadata only (asset
      bytes are out of scope).
- [x] Document the idiomatic read + write API (B4/B7): the typed borrowed
      accessors (`inventory_root` / `library_root`, `inventory_folder` /
      `inventory_item`, the `Child`-yielding `inventory_children`,
      `folder_fetch_state`, the paged `inventory_folder_page` +
      `InventoryCursor`), the owning view types `FolderInfo` / `ItemInfo` with
      resolved enums, and the deprecation of the raw `inventory_folders()` /
      `inventory_items()` maps.
- [x] Document background-fetch orchestration (B6): the opt-in
      `set_background_inventory_fetch` crawl, `next_inventory_fetch_batch` /
      `request_folder_contents`, and the `inventory_fully_loaded` completion
      signal.
- [x] Document the `Command`/`Event` pull-bridge (B8):
      `Command::QueryInventoryFolder` / `Command::QueryInventoryRoots` answered
      by the **synthesized-locally** `Event::InventoryFolderPage` /
      `Event::InventoryRoots`, the `Arc<[…]>` copy budget, and the bevy
      zero-copy `&Session` borrow alternative.
- [x] Document the disk cache (B9/B10): the Firestorm-compatible
      `<agent-uuid>.inv.llsd.gz` / `.lib.inv.llsd.gz`, the version-gated
      load → merge-with-skeleton flow, `ClientDirectories`, and that the cache
      is grid-level (survives teleport / region crossings).
- [x] Gate: `mdbook build book`, `rumdl` on the chapter (80-col), on the current
      branch.
