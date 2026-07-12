# Context — INVENTORY_ROADMAP.md

Non-task preamble from `INVENTORY_ROADMAP.md` (scope, protocol/implementation
reality, locked decisions, and Phase-B consolidation notes). Tasks split out of
that file carry the `inventory` topic; each Phase-A design item folds in its own
`reference (from A*)` section.

A plan to give the SL client a *stateful*, **two-level cached** inventory system
— the agent's own inventory tree **and** the read-only SL Library tree — held in
`Session` for the library user to navigate idiomatically, and persisted to disk
between runs the way the reference viewer (Firestorm) does. Today the wire
plumbing is complete (typed keys, domain types, the UDP
`FetchInventoryDescendents` / `InventoryDescendents` / `BulkUpdateInventory`
messages, the modern `FetchInventoryDescendents2` / `InventoryAPIv3` /
`LibraryAPIv3` / `CreateInventoryCategory` CAPS, the LLSD converters, the
commands and the `InventorySkeleton` / `InventoryDescendents` /
`InventoryItemCreated` / `InventoryBulkUpdate` events), and `Session` even keeps
a **basic live cache** (`inventory_folders` / `inventory_items` /
`inventory_root` `BTreeMap`s, kept current by the agent's own mutations) reached
through the `inventory_root` / `inventory_folder` / `inventory_item` /
`inventory_folders` / `inventory_items` / `inventory_children` accessors. What
is **missing** — and what this roadmap delivers — is the layer *above* that
plumbing: a proper held read-model with an idiomatic, typed, borrowed, paginated
public API; a **version-gated** memory cache that knows which folders are loaded
vs stale; a **disk cache** (Firestorm-compatible `<agent-uuid>.inv.llsd.gz`) so
a relogin reuses unchanged folders instead of refetching the whole tree; and the
**Library** tree held the same way. Work these top-to-bottom; tick a box only
when the step builds, is clippy-clean (restriction lints), and `cargo test`
passes. Add sub-tasks as you discover them.

Phase A is **planning only** — its items produce design decisions, not code;
those decisions are recorded in the **§ Phase A design references** below (one
per A-item), which are the actual output of Phase A. Phase B (implementation,
B1–B12) is the task list **derived from** those references, executed once Phase
A is signed off. Tick a Phase A box to sign off the matching reference.

Scope reminders:

- Commit on the current branch only (never auto-create a feature branch).
- `Session` (sl-proto) is sans-IO: the held inventory model, fetch-state, and
  the pure cache (de)serialisation + merge logic live there, beside the chat
  read-model; **no filesystem, no gzip, no clock** in sl-proto.
- File I/O, gzip, and the caller-supplied cache directory live in the runtime
  shells (`sl-client-tokio` / `sl-client-bevy`), mirroring the `chat_log` split.
- Keep `sl-client-tokio` and `sl-client-bevy` (and the REPL) at feature parity.
- Never push client-only protocol types into the shared `sl-types` crate.
- Prefer raw types on the wire and idiomatic Rust (newtypes, structs, lifetimes,
  borrowing) in the user-facing API — typed keys, never bare `Uuid`, at the
  surface; the existing raw `&BTreeMap<Uuid, …>` accessors are **deprecated**
  and replaced.
- Wrap this file at 80 columns; fmt/clippy/rumdl green before commit (the ggh
  hook rejects MD013 and re-runs clippy).

## Decisions locked with the user (before Phase A sign-off)

- **Disk format:** byte-compatible **binary LLSD** — the cache file is
  `<agent-uuid>.inv.llsd.gz`: a 4-byte big-endian version header (= `5`,
  matching Firestorm `sCurrentInvCacheVersion`), then a binary-LLSD map
  `{ "categories": [...], "items": [...] }`, gzipped — readable by Firestorm and
  vice-versa. This requires a new binary-LLSD **encoder + decoder** (only
  XML-LLSD exists today).
- **LLSD crate:** the LLSD core is **extracted** from `sl-wire` into a new
  foundational `sl-llsd` workspace crate (above `sl-types`, below `sl-wire`) as
  the first Phase B task, so the binary codec lands in its natural home and any
  consumer can depend on LLSD without all of `sl-wire`. sl-wire keeps a
  re-export shim so the ~24 dependents are unchanged.
- **API scope:** a full idiomatic overhaul — typed/borrowed tree-walk accessors,
  snapshot view types, pagination, and a `Command`/`Event` pull-bridge for the
  channel-based runtimes; the raw-`Uuid` accessors are deprecated.
- **Scope:** hold + cache **both** the agent inventory **and** the read-only SL
  Library tree (`LibraryAPIv3` / `FetchLibDescendents2`), each with its own
  owner/root and its own cache file.
- **Directories:** the caller supplies every directory **verbatim** (no
  auto-derived per-agent subdir), bundled into one `ClientDirectories` struct
  passed once at construction — `agent_cache_dir` and `agent_chat_log_dir` (each
  `Option<PathBuf>`, `None` disables that feature), with a `shared_cache_dir`
  slot reserved for a future asset/texture cache.
  **Chat logging is retrofitted** to the same scheme (it takes its directory
  verbatim, dropping the current auto per-account subdir).

## Implementation reality (constraints Phase A must respect)

- The inventory **wire layer is done**. Reuse, do not rebuild: the converters in
  `sl-proto/src/session/conversions.rs` (`inventory_descendents_from_llsd`,
  `inventory_folder_from_llsd`, `inventory_item_from_llsd`, the matching
  `_to_llsd` builders, `bulk_update_inventory_from_llsd`,
  `ais_inventory_update_from_llsd`), `build_fetch_inventory_request` (sl-wire),
  and the CAPS constants (`CAP_FETCH_INVENTORY` =
  `"FetchInventoryDescendents2"`, `CAP_INVENTORY_API_V3`, `CAP_LIBRARY_API_V3`,
  `CAP_CREATE_INVENTORY_CATEGORY` in `session.rs`).
- `Session` already holds `inventory_folders: BTreeMap<Uuid, InventoryFolder>`,
  `inventory_items: BTreeMap<Uuid, InventoryItem>`,
  `inventory_root: Option<InventoryFolderKey>` and `next_inventory_callback`,
  filled by the private `cache_inventory` / `cache_inventory_folder` /
  `cache_inventory_item` helpers at the `InventoryDescendents` /
  `InventoryBulkUpdate` / `InventoryItemCreated` arms (`methods.rs` ~400–470,
  ~2406–2470) and by the login skeleton (`methods.rs:1187`, `:1223`). This is
  the seed the model module folds in — it is **not** version-aware, has
  **no parent→children index**, and exposes raw `Uuid` maps.
- `InventoryFolder.version` exists but is only `0` for sub-folders of a
  descendents reply; the login skeleton carries the real per-folder version. The
  viewer's cache validity rests entirely on comparing a cached folder's version
  to the server/skeleton version — so the model must record the
  **authoritative** version per folder and a per-folder **fetch state**, not a
  bare map.
- LLSD today is XML-only and lives **inside** `sl-wire` (`sl-wire/src/llsd.rs`,
  1318 lines): `Llsd` (11 variants — `Undef`, `Boolean`, `Integer`, `Real`,
  `String`, `Uuid`, `Date`, `Uri`, `Binary`, `Array`, `Map`) with `to_llsd_xml`
  / `parse_llsd_xml`, plus a notation reader in `material/gltf.rs`. It is used
  by 20 sl-wire modules and by `sl-proto` / the runtime crates. B1 extracts it
  into a foundational `sl-llsd` crate (above `sl-types`, below `sl-wire`) before
  B2 adds the binary tag-byte codec there — see the A3 reference. The current
  `sl-wire/src/llsd.rs` is **not** self-contained (it imports sl-wire's
  `WireError` and three `sl_types` keys), so the cut splits out an `LlsdError`.
- Inventory, like chat, is **grid-level**: routed by the grid's inventory
  service, not the region simulator. It **persists** across teleport / region
  crossings — the *inverse* of `SitState` / script-permission resets — so the
  region-boundary handlers must **not** clear it (the chat persistence-guard
  precedent, `CHAT_ROADMAP.md` B10).
- The Library tree is a **second** owner (`AgentPreferences` / login response
  carries the library-owner id and library-root id) fetched read-only over
  `FetchLibDescendents2` (UDP `FetchInventoryDescendents` with the library
  owner), and over `LibraryAPIv3` on SL. It is large but nearly immutable, so it
  caches especially well; it is keyed and persisted **separately** from agent
  inventory.
- The `chat_log` feature is the precedent for the whole runtime split: a pure
  format/config core in `sl-proto` (`chat_log.rs`, `ChatLogConfig`) and a
  file-I/O shell in each runtime (`sl-client-tokio/src/chat_log.rs`,
  `ChatLog::new`). The read-model exposure precedent (typed snapshot views +
  cursor pagination + a query/reply pull-bridge for the channel runtimes, direct
  `&Session` borrow for bevy) is `ChatSessionInfo` / `MessageCursor` /
  `history_page` / `Command::QueryChatHistoryPage` / `Event::ChatHistoryPage`.

## Other notes

## Phase B tasks — consolidated (B1–B12)

Derived from the signed-off § Phase A design references and dependency-ordered
so each task leaves the tree buildable, clippy-clean (restriction lints), and
`cargo test`-green on its own (sl-proto's `[lints.rust]` denies the `unused_*`
family and the ggh pre-commit re-runs full clippy every attempt, so every
field/type lands **with** its writer, its reader, and its tests). Each task
names the reference it implements. Keep `sl-client-tokio`, `sl-client-bevy`, and
the REPL at parity; never push client-only types into shared `sl-types`.
**Ask the user before starting Phase B** (the standing "ask before new roadmap
work" rule).
