---
id: inventory-b8
title: Command/Event pull-bridge
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B8. `Command`/`Event` pull-bridge (from A8)

- [x] Add `Command::QueryInventoryFolder { folder, before, limit }` +
      `Command::QueryInventoryRoots` in `sl-proto/src/command.rs`; add
      `Event::InventoryFolderPage` (carrying
      `{ folder, folders: Arc<[FolderInfo]>, items: Arc<[ItemInfo]>, prev }`)
      and `Event::InventoryRoots { agent_root, library_root }` in
      `sl-proto/src/types/event.rs`, each documented "**synthesized locally**"
      like `Event::ChatHistoryPage` (`event.rs:919-930`). **No `methods.rs`
      dispatch arm** — the bridge mirrors `QueryChatHistoryPage`, which is
      answered in the runtimes, not in sl-proto.
- [x] Synthesize each reply in **both** runtimes' command-dispatch arms beside
      the existing `QueryChatHistoryPage` ones (tokio `lib.rs:1622-1650`, bevy
      `lib.rs:2716-2748`): call the pure `inventory_folder_page` (B4) /
      `inventory_root()` / `library_root()` read methods on `Session` and push
      the `Arc<[…]>`-payload event onto the stream. Wire the REPL query/page
      beside its chat-history wiring (`sl-repl/src/registry.rs:5240`,
      `sl-repl-tokio/src/bin/sl-repl-tokio.rs:504`/`:597-600`). A bevy reader
      system may additionally borrow `&Session` and call `inventory_folder_page`
      directly (zero-copy); the bridge arm stays for parity.
- [x] Tests: a folder query replies with the paged view-type window + cursor; an
  `Unknown`-folder query schedules its fetch (ties to B6); the page payloads are
  `Arc<[…]>`-shaped and the direct-borrow `inventory_children` reader compiles
  against `&Session` with no cache clone (the no-unnecessary-copy budget).
