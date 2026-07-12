---
id: inventory-a8
title: Design the Command/Event pull-bridge & runtime divergence
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A8. Design the `Command`/`Event` pull-bridge & runtime divergence.** For
    the channel runtimes (tokio / REPL) that move `Session` into a task: a
    query command and a reply event — e.g. a `Command::QueryInventoryFolder`
    carrying `{ folder, before: Option<InventoryCursor>, limit }`, answered by
    an `Event::InventoryFolderPage` carrying the `folder`, its
    `folders: Arc<[FolderInfo]>` and `items: Arc<[ItemInfo]>` window, and the
    next `prev: Option<InventoryCursor>` — plus a roots/summary query and
    cache load/save signals. bevy reads the model directly via `&Session`
    borrow (zero-copy, no round-trip). Parity = identical data / commands /
    view types; only the read *transport* differs. Wire the new command across
    the tokio / bevy / REPL dispatch sites at parity.

## Pull-bridge & runtime-divergence reference (from A8)

Commands `Command::QueryInventoryFolder { folder, before, limit }` and
`Command::QueryInventoryRoots` (read queries); on-demand *fetch* reuses the
existing `Command::RequestFolderContents(InventoryFolderKey)` (B6, not a new
command). Replies `Event::InventoryFolderPage { folder, folders:
Arc<[FolderInfo]>, items: Arc<[ItemInfo]>, prev }` and `Event::InventoryRoots {
agent_root, library_root }`. **Each reply event is *synthesized locally* by the
runtime that owns `Session`, not dispatched by `methods.rs`** — the runtime's
command-dispatch arm calls the pure read method on `Session`
(`inventory_folder_page` from A7/B4 for the page query; the typed
`inventory_root()` / `library_root()` accessors for the roots query) and pushes
the assembled `Event` onto its own event stream, exactly as the chat bridge does
for `QueryChatHistoryPage`. tokio / REPL go through this bridge (`Arc<[…]>`
snapshots, O(1) hand-off); a bevy reader *system* may instead borrow `&Session`
and call `inventory_folder_page` directly, skipping the round-trip — but bevy
still implements the bridge arm too, for parity. Parity = identical data /
commands / view types; only the read *transport* differs. Wire every new command
across the tokio / bevy / REPL dispatch sites.

**Verified against the code (anchors for B8).** The pull-bridge is the
**chat-history precedent**, and that precedent does **not** route the query
through `sl-proto`: `Command::QueryChatHistoryPage { session, before, limit }`
(`command.rs:2221`) is answered by `Event::ChatHistoryPage { session, messages:
Arc<[SessionMessage]>, prev }` (`event.rs:931`) that is **synthesized locally**
in *both* runtimes' command-dispatch arms — tokio
`lib.rs:1622-1650` and bevy `lib.rs:2716-2748` — each calling the pure
`Session::history_page(session, before, limit)` (`methods.rs:5057`,
`-> (impl Iterator<Item = &SessionMessage>, Option<MessageCursor>)`) and writing
the event onto its own stream (no `methods.rs` dispatch arm exists for it —
grep-confirmed; the `Event` doc at `event.rs:919-930` states "**synthesized
locally**", as do the sibling `Event::ScriptPermissionState` `:909-913`,
`Event::ChatSessions` `:919-924`, `Event::FriendsSnapshot` `:940-944`). So B8's
`Event::InventoryFolderPage` / `Event::InventoryRoots` carry the **same
"synthesized locally"** doc note and the same `Arc<[…]>` payload shape (an `Arc`
clone across the channel, never a deep copy), and B8 adds **no** `methods.rs`
arm — only the per-runtime synthesis arms beside the existing
`QueryChatHistoryPage` ones.

The four new symbols do **not** exist yet (grep-confirmed):
`Command::QueryInventoryFolder`, `Command::QueryInventoryRoots`,
`Event::InventoryFolderPage`, `Event::InventoryRoots`. The existing inventory
**Command**s B8 sits beside are
`Command::RequestFolderContents(InventoryFolderKey)`
(`command.rs:168`, the on-demand single-folder fetch reused here) and
`Command::FetchInventoryFolders(Vec<InventoryFolderKey>)` (`command.rs:172`);
the existing inventory **Event**s are `InventorySkeleton` (`event.rs:459`),
`LibraryInventory` (`:465`), `InventoryDescendents` (`:469`),
`InventoryItemCreated` (`:486`), `InventoryBulkUpdate` (`:503`). The central
command-dispatch sites the new arms join are tokio `lib.rs:410` (`match command`
in the select loop; existing inventory arms `RequestFolderContents` `:512` /
`FetchInventoryFolders` `:515`) and bevy `lib.rs:529` (`match &command.0` in
`advance_running`, reached from the `drive` system; existing inventory arms
`:642`/`:645`). The REPL builds the command from its registry
(`sl-repl/src/registry.rs`, the `QueryChatHistoryPage` spec at `:5240`) and
consumes the reply event in the formatter loop
(`sl-repl-tokio/src/bin/sl-repl-tokio.rs`, command send `:504`, event apply
`:597-600`) — B8 adds the inventory query/page beside those.

**Read-method readiness (ordering).** The page query's pure read method
`inventory_folder_page(folder, before, limit) -> (Vec<FolderInfo>,
Vec<ItemInfo>, Option<InventoryCursor>)` and the `library_root()` accessor are
**B4** deliverables (A7); the `InventoryCursor` it returns is the cursor B8
carries across the `Command`/`Event` boundary (its `pub from_consumed` /
`consumed_count`, the `MessageCursor` precedent). So B8 depends on B4 (and
transitively B3) being in place — it is pure wiring over an already-built read
API, the same layering as the chat bridge over `history_page`.

**No unnecessary copies of the in-memory cache (chat parity, user
requirement).** The held model is the single source of truth and the bridge must
not deep-copy it without need — mirroring the chat read-model's copy budget:

- **Zero-copy borrow path is primary.** A reader holding `&Session` — bevy
  systems, and any in-task caller — reads the cache **borrowed**, never copied:
  the A7/B4 `inventory_children(folder) -> impl Iterator<Item = Child<'_>>`
  (yielding `&InventoryFolder` / `&InventoryItem` straight out of the stores)
  and the by-key `inventory_folder` / `inventory_item` lookups. This is the
  `chat_sessions_info()` / `history_page()` borrowed-iterator precedent: bevy
  takes it and **skips the bridge entirely** (the bridge arm exists only for
  parity).
- **One copy, only at the channel boundary, only when ownership is mandatory.**
  The owning view types (`FolderInfo` / `ItemInfo`, resolving `i8`/`u8` → typed
  enums) are materialised **only** when the page must cross the command/event
  channel to a runtime that has moved `Session` into its task (tokio / REPL) and
  therefore cannot borrow. That single transform is unavoidable (it leaves the
  borrow's lifetime and resolves the enums); B8 does it once, into the bounded
  page window — never the whole tree, never per delivery.
- **`Arc<[…]>` so re-handoff is a refcount bump.** The payload is
  `folders: Arc<[FolderInfo]>` / `items: Arc<[ItemInfo]>` (matching
  `Event::ChatHistoryPage`'s `Arc<[SessionMessage]>`): handing the page across
  the channel, cloning the `Event`, or redelivering the same page is an `Arc`
  clone, **never** a second deep copy of the window. `Event::InventoryRoots`
  carries two `Copy` `InventoryFolderKey`s — trivially cheap, no `Arc` needed.

So the copy budget per page query is exactly *one* bounded borrowed→owned
transform on the channel runtimes and *zero* for the direct-borrow reader —
identical to chat. B8's tests assert the borrow path compiles against `&Session`
with no clone and the page payloads are `Arc`-shaped.
