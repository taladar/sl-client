---
id: inventory-b10
title: Runtime cache shells (tokio + bevy)
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B10. Runtime cache shells (tokio + bevy) (from A4·A5·A10) — DONE

- [x] Added an `inventory_cache.rs` runtime shell (byte-identical, mirroring the
      `chat_log.rs` precedent) to each of `sl-client-tokio` and
      `sl-client-bevy`: locates `<agent-uuid>.inv.llsd.gz` / `.lib.inv.llsd.gz`
      **directly** under `agent_cache_dir` (`None` ⇒ caching disabled); gzips
      via the new `flate2` dep (added to both runtime crates). Blocking I/O (a
      `fs_err` + `flate2` atomic write), not the chat append pattern — this
      crash-safe gzip write is all-new code. **Crash-safe atomic write per A4:**
      streams the gzip to a same-directory `…<pid>.tmp`, flush + `fsync`
      (`File::sync_all`), then atomic `rename` over the target; removes the temp
      + keeps the old cache on any error.
- [x] `InventoryCacheConfig` (master enable flag + library-cache toggle, in
      `sl-proto` beside `ClientDirectories`, re-exported from both runtimes;
      default OFF, library toggle ON) consumed beside the dir from
      `ClientDirectories`. Loads at the `InventorySkeleton` / `LibraryInventory`
      event (before the merge) → calls the B5 `merge_inventory_skeleton` → the
      reconciled `Unknown` set **is** B6's fetch queue (no separate injection);
      saves on the terminal event (logout) and on a dirty/idle tick using the B5
      cacheable snapshot (`Loaded` folders only). The dirty/idle save is gated
      on a new sans-IO `Session::inventory_dirty()` / `clear_inventory_dirty()`
      (an `Inventory.dirty` flag set by every fold/mutation), so an unchanged
      model is never needlessly rewritten — the optional crash-safety tick
      beyond Firestorm's shutdown-only save.
- [x] Tests: a caller-supplied temp dir round-trips save → gunzip → 4-byte
      header `5` → load → merge → model equality (a `Loaded` folder survives the
      round-trip at its version), the Firestorm-shaped `category_id` key is
      present in the bytes, plus disabled-config and library-toggle-off gating
      tests (in both runtime crates).
