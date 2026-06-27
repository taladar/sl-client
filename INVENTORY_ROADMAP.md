# inventory road map

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
B1–B11) is the task list **derived from** those references, executed once Phase
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

## Phase A — plan the held-inventory + cache system (design only; no code yet)

Each item is a design directive; its pinned-down output lives in the matching
**§ … reference (from A#)** under § Phase A design references below. Ticking an
item's box signs off that reference. Phase B (B1–B10) is then the implementation
task list derived from the signed-off references.

- [x] **A1. Inventory the surface & define the held model.** Enumerate what
      exists (the wire layer, the raw maps, the `cache_inventory*` folds, the
      four events) and define the unified held model: an `Inventory` value
      owning the folder store, the item store, the **roots** (agent root +
      library root), and an **owner discriminator** (`Agent` vs `Library`) so
      the two trees share one model but stay queryable apart. State the
      boundary: this roadmap holds + caches **structure/metadata** (folders +
      items); **asset bytes** (textures, meshes, notecard/script contents) are a
      separate concern, OUT of scope (a future `shared_cache_dir` asset cache).
      Confirm the typed keys (`InventoryKey`, `InventoryFolderKey`,
      `InventoryItemOrFolderKey`) and domain types (`InventoryFolder`,
      `InventoryItem`) are reused unchanged underneath.
- [x] **A2. Design the fetch-state machine & the parent→children index.**
  Specify a per-folder fetch state — `Unknown` (in the tree from skeleton/parent
  but contents not fetched) / `Fetching` (request in flight) / `Loaded { version
  }` (contents present, known version) — and the authoritative-version rule: a
  folder's version comes from the skeleton or a descendents reply, and `Loaded`
  is only entered with that version. Design the parent→children index (folder id
  → child folder ids + child item ids) maintained incrementally by every fold
  site so children/tree-walks are O(children), not O(whole tree). Define how the
  existing `cache_inventory*` folds update the index + fetch-state.
- [x] **A3. Extract the `sl-llsd` crate & specify the binary codec.** Pull the
      LLSD core (`Llsd` + the XML codec + the notation reader now in
      `material/gltf.rs`) out of `sl-wire` into a new foundational `sl-llsd`
      workspace crate (depending only on `sl-types` + `uuid` + `base64` +
      `roxmltree` + `time`), with a crate-local `LlsdError` (and
      `From<LlsdError> for WireError` back in sl-wire) and a
      `pub use sl_llsd as llsd` re-export so the ~24 dependents keep compiling.
      Then add the new binary codec there: `Llsd::to_llsd_binary() -> Vec<u8>`
      and `parse_llsd_binary(&[u8]) -> Result<Llsd, LlsdError>` against the LL
      binary-LLSD tag bytes (`!` undef, `1`/`0` boolean, `i` i32 BE, `r` f64 BE,
      `s`+len+utf8 string, `u`+16 bytes uuid, `d`+f64 date, `l`+len+utf8 uri,
      `b`+len+bytes binary, `[`+count array, `{`+count map with `k`+len+key
      entries). The gzip envelope wraps the whole file *including* the 4-byte
      version header (Firestorm `saveToFile` writes header + binary LLSD to a
      temp file, then gzips it). Round-trips every `Llsd` variant and the real
      cache map. → see § `sl-llsd` extraction & binary-codec reference (from
      A3).
- [x] **A4. Design the disk-cache file layout & the `ClientDirectories`
      struct.** File naming (`<agent-uuid>.inv.llsd.gz` for agent, a distinct
      `<agent-uuid>.lib.inv.llsd.gz` for library) placed **directly** in the
      caller-supplied directory (no derived subdir); atomic write (temp file +
      gzip + rename); the version-header check on load (mismatch ⇒ ignore the
      file). Define `ClientDirectories` (three `Option<PathBuf>` fields —
      `agent_cache_dir`, `agent_chat_log_dir`, and a reserved
      `shared_cache_dir`) in `sl-proto` (next to `ChatLogConfig`), passed once
      at construction, and the **chat-log retrofit**: `ChatLog::new` takes
      `agent_chat_log_dir` verbatim and drops the `clean_file_name(own_name)`
      subdir join. Specify what changes in the existing chat-log tests.
- [x] **A5. Design cache load / merge-with-skeleton / version-validity.** The
      login sequence: load the disk cache (if present + version-valid) into the
      model, then receive the skeleton, then **merge** — for each skeleton
      folder, if the cached version equals the skeleton version keep the cached
      contents and mark `Loaded`, else drop the cached contents and mark
      `Unknown` (refetch); add skeleton folders absent from the cache as
      `Unknown`; drop cached folders absent from the skeleton (deleted
      server-side). Items are kept only under a folder that stayed `Loaded`.
      This logic is **pure** (`sl-proto`, no I/O): it takes the loaded model +
      the skeleton and returns the merged model + the set of folders needing
      fetch. Mirror Firestorm `loadSkeleton`.
- [x] **A6. Design background-fetch orchestration.** A breadth-first walk over
      folders in state `Unknown`, emitting `FetchInventoryDescendents2` requests
      (agent tree) and `FetchLibDescendents2` (library tree), throttled/batched
      (a bounded number in flight, like Firestorm's background fetch), flipping
      folders `Unknown → Fetching → Loaded` as replies land. The *decision* of
      what to fetch next is sans-IO in `sl-proto` (a method returning the next
      batch of folder ids); the *I/O* (the CAPS POST) is the runtime shell,
      reusing the existing fetch path. Define the completion signal (tree fully
      `Loaded`) and the on-demand path (a user query for an `Unknown` folder
      triggers its fetch). **The automatic crawl must be disable-able** (an
      opt-out flag) so a consumer that never reads inventory — e.g. the
      `sl-survey` binary, which ignores every inventory event — pays nothing;
      on-demand fetch stays available regardless of the flag.
- [x] **A7. Design the idiomatic public model API (read + write).** *Read:*
      borrowed, typed tree-walk accessors on `Session`:
      `inventory_root`/`library_root` (typed `InventoryFolderKey`), a `Vec`-free
      `inventory_children(folder) -> impl Iterator` yielding a `Child` enum
      (`Folder(&InventoryFolder)` / `Item(&InventoryItem)`) or split folder/item
      iterators, a folder/item lookup by typed key,
      `folder_fetch_state(folder)`, and snapshot **view types** (`FolderInfo` /
      `ItemInfo`, owning, `Arc`-friendly, exposing typed keys + resolved enums
      like `AssetType` / `InventoryType` / `SaleType` instead of raw `i8`/`u8`).
      Pagination for large folders via an opaque `InventoryCursor` (the
      `MessageCursor` precedent). Deprecate `inventory_folders()` /
      `inventory_items()` (raw `&BTreeMap<Uuid, …>`). *Write:* make the existing
      mutation surface symmetric with the read side — accept the same resolved
      enums (`FolderType` / `AssetType` / `InventoryType` / `WearableType`) on
      writes instead of raw `i8`/`u8`, surface caller mistakes as `Error` (nil /
      duplicate folder id, a move that would form a cycle or target an unknown
      parent — all O(1) against the B3 index), add focused clobber-free helpers
      (rename / re-type / set-permissions reading the other fields from the
      cache) so an all-fields `UpdateInventory*` cannot accidentally re-parent
      or reset permissions, and pin the optimistic-write → authoritative-reply
      reconciliation policy (the optimistic edit sets state + `dirty` with a
      guessed version, overwritten last-write-wins when the server reply folds
      in). The cache-coherence *mechanics* (index / `FolderState` / version at
      every mutation site) stay owned by A2/B3; A10 owns `dirty`.
- [x] **A8. Design the `Command`/`Event` pull-bridge & runtime divergence.** For
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
- [x] **A9. Design Library-inventory holding & caching.** Source the library
      owner id + library root from the login response; hold the library tree in
      the same `Inventory` model under `owner = Library` with its own root;
      fetch it over `FetchLibDescendents2` / `LibraryAPIv3` (read-only — no
      mutation commands target it); persist it to the separate
      `<agent-uuid>.lib.inv.llsd.gz` with the same version-gated validity.
      Decide the library fetch policy (lazy on first library query vs eager
      background) given its size.
- [ ] **A10. Design persistence-vs-region & lifecycle.** Confirm the model is
  **not** cleared at the four region-boundary sites (it is grid-level — the chat
  persistence-guard precedent). Define save points: on logout and on an
  idle/dirty interval (the model tracks a dirty flag set by any fold/mutation),
  collecting only `Loaded` folders + their items (skip `Unknown`/`Fetching`),
  matching Firestorm `LLCanCache`. Define load timing (at/just-before the
  skeleton). The sans-IO `Session` exposes "give me the cacheable snapshot" /
  "load this snapshot"; the runtime shell does the file/gzip I/O around it.
- [ ] **A11. Test & verification strategy.** Extend the existing inventory tests
      in `sl-proto/tests/lifecycle.rs` (and the `inventory_edit` example) rather
      than duplicating: skeleton seeds roots + `Unknown`; a descendents reply
      flips a folder to `Loaded` and fills the index; binary-LLSD round-trip
      (every variant + the cache map); merge keeps a version-matching folder (no
      refetch) and drops a stale one; pagination walks a large folder; the model
      **survives teleport** (mirror the chat persistence test); the library tree
      is held under its own root; the chat-log verbatim-dir retrofit. Runtime: a
      caller-supplied temp dir round-trips save → gunzip → header `5` → load →
      merge → model equality. Live: OpenSim relogin reuses unchanged folders.

## Phase A design references (drafted — sign-off pending)

The detailed output of each Phase A item — the concrete types, signatures, and
algorithms the Phase B tasks build to. Drafted here for review; ticking an
A-item above signs off its reference.

### Held-model reference (from A1)

A single `Inventory` value (new `sl-proto/src/session/inventory.rs`) replaces
the three loose `Session` fields (`inventory_folders` / `inventory_items` /
`inventory_root`) and both trees live in it. It owns a folder store keyed by
`InventoryFolderKey` and an item store keyed by `InventoryKey` (typed keys, not
the raw `Uuid` maps of today), the two roots (`agent_root` /
`library_root: Option<InventoryFolderKey>`), the
`library_owner: Option<OwnerKey>` (for library fetches),
`next_inventory_callback`, and a `dirty: bool` (A10). Each folder is wrapped in
a `FolderEntry` carrying the `InventoryFolder` payload, an `InventoryOwner`
(`Agent` / `Library`), its `FolderState` (A2), and its child-key sets (A2); each
item in an `ItemEntry` carrying the `InventoryItem` and its owner. The domain
types `InventoryFolder` / `InventoryItem` and the typed keys (`InventoryKey` /
`InventoryFolderKey` / `InventoryItemOrFolderKey`) are reused unchanged as
payloads. Boundary: structure/metadata only — **asset bytes** (textures, meshes,
notecard/script contents) stay out of scope (a future `shared_cache_dir` asset
cache).

**Surface verified against the code (anchors for B3).** The three loose fields
are `session.rs:1002` (`inventory_root: Option<InventoryFolderKey>`),
`session.rs:1082` (`inventory_folders: BTreeMap<Uuid, InventoryFolder>`),
`session.rs:1088` (`inventory_items: BTreeMap<Uuid, InventoryItem>`), plus
`session.rs:1091` (`next_inventory_callback: InventoryCallbackId`). The folds
are
`cache_inventory_folder` (`methods.rs:7897`), `cache_inventory`
(`methods.rs:7910`), `cache_inventory_item` (`methods.rs:7920`), reached from
the `InventoryDescendents` arm (`methods.rs:2414`), the
`UpdateCreateInventoryItem`
arm (`methods.rs:2432` → `Event::InventoryItemCreated`), and the
`BulkUpdateInventory` arm (`methods.rs:2467`). The login skeleton seeds the map
at `methods.rs:1216-1226` via `skeleton_folder` (`conversions.rs:957`), which
already copies the authoritative `version` per folder; and
`cache_inventory_folder` already preserves an existing version when a fold
carries `0` (the seed of the A2 authoritative-version rule). Current accessors:
`inventory_root` (`methods.rs:7812`, typed), `inventory_folder`/`inventory_item`
(`:7848`/`:7855`, typed lookup), the raw `inventory_folders`/`inventory_items`
(`:7861`/`:7867`, `&BTreeMap<Uuid, …>`, to be deprecated in B4), and
`inventory_children` (`:7876`, returns `(Vec<&InventoryFolder>,
Vec<&InventoryItem>)` by an O(tree) `parent_id`/`folder_id` scan — the index in
A2 replaces this). The domain types live in `types/inventory.rs`
(`InventoryFolder` ll.13-25 with `version: i32` + `folder_type: i8`,
`InventoryItem` ll.29-69 with `item_type`/`inv_type: i8`, `sale_type: u8`,
`asset_id: Uuid`, `Permissions5`); the typed keys are newtypes in
`sl-types/src/key.rs` (`InventoryKey` l.306, `InventoryFolderKey` l.435,
`InventoryItemOrFolderKey` l.995, `OwnerKey` l.471) — all reused unchanged.

**Two cross-references for A9/B7 discovered while enumerating.** (1) The login
response carries the library identifiers. `library_root` was already typed
(`Option<InventoryFolderKey>`) but `library_owner` was a bare `Uuid` — fixed in
this commit to `Option<AgentKey>` in both `sl-wire` `LoginSuccess`
(`login.rs:393`) and `sl-proto` `LoginAccount` (`avatar_profile.rs:353`): the
wire field is `inventory-lib-owner`/`agent_id`, always an avatar (never a
group), so `AgentKey` is the tight fit (`OwnerKey` would admit an impossible
`Group`
arm). The held model's `library_owner: Option<OwnerKey>` widens it to `OwnerKey`
only where the fetch path wants a uniform owner type. (2) A library skeleton
event **already exists**:
`Event::LibraryInventory` is emitted at `methods.rs:1213` alongside
`InventorySkeleton`; A9/B7 fold its folders into the model under
`owner = Library` rather than introducing a new event.

### Fetch-state & index reference (from A2)

`pub enum FolderState { Unknown, Fetching, Loaded { version: i32 } }`.
Authoritative-version rule: a folder enters `Loaded` only with a version from
the login skeleton or a descendents reply *for that folder*; a sub-folder that
appears merely as a child in some other folder's descendents reply (wire
`version 0`) stays `Unknown` until fetched in its own right. The parent→children
index is the child-key sets on `FolderEntry`
(`child_folders: BTreeSet<InventoryFolderKey>`,
`child_items: BTreeSet<InventoryKey>`), maintained at every fold:
`cache_inventory_folder` inserts the folder, links it under its `parent_id`'s
entry, and (if new) creates its entry `Unknown`; `cache_inventory_item` inserts
the item and links it under its `folder_id`; removals unlink. So
`inventory_children(folder)` is O(children), not O(tree).

**Fold / unlink sites verified against the code (anchors for B3).** Every site
that mutates `inventory_folders` / `inventory_items` must maintain the index +
fetch-state. Inserts/updates flow through `cache_inventory_folder`
(`methods.rs:7897`) and `cache_inventory_item` (`:7920`) — both via
`cache_inventory` (`:7910`) — the natural index hooks. **But two classes of site
bypass them and B3 must route through the model too:** (1) the login **skeleton
seed inserts directly** (`:1223-1224`, a raw `inventory_folders.insert`, *not*
`cache_inventory_folder`) — this is the one site carrying the authoritative
per-folder `version`, so it must set `Loaded`/`Unknown` + index *there*, not
lose it; (2) the **re-parent mutations edit the parent link in place** —
`move_inventory_folders` mutates `folder.parent_id` (`:8062`) and
`move_inventory_items` mutates `item.folder_id` (`:8193`) — so the index must
*unlink from the old parent and link to the new* at these two sites (an
insert/remove pair does not model a move). Unlink/removal sites:
`purge_cached_descendents` (`:7926`, recursive), `remove_inventory_folders`
(`:8085-8086`), `remove_inventory_items` (`:8248`),
`purge_inventory_descendents` (`:8288`), and `remove_inventory_objects`
(`:8310-8314`) — each unlinks the
dropped keys from their parent's child-sets. The optimistic creates
(`create_inventory_folder` `:7978` version `1`, the AIS folder create `:8011`)
already route through `cache_inventory_folder`, so they index for free once the
hook is there.

Authoritative-version anchor: `cache_inventory_folder` **already** preserves an
existing version when a fold carries `0` (`:7898-7903`) — the seed of the
`Loaded` rule; B3 promotes that ad-hoc guard into `FolderState` so a `version 0`
child fold leaves the entry `Unknown` rather than fabricating `Loaded { 0 }`.

### `sl-llsd` extraction & binary-codec reference (from A3)

**Extraction first.** Create a new `sl-llsd` workspace member holding the LLSD
core: the `Llsd` enum, the XML codec (`to_llsd_xml` / `parse_llsd_xml`), the
notation reader currently in `sl-wire/src/material/gltf.rs`, and the typed-key
convenience accessors. It depends only on `sl-types`, `uuid`, `base64`,
`roxmltree`, and `time` — sitting **above** `sl-types` and **below** `sl-wire`
in the graph (no cycle). The current `sl-wire/src/llsd.rs` imports
`crate::error::WireError`, so the move introduces a crate-local `LlsdError` and
an `impl From<LlsdError> for WireError` in sl-wire. sl-wire keeps a
`pub use sl_llsd as llsd` re-export (and re-exports the moved free functions) so
the 20 sl-wire modules and the downstream `sl-proto` / runtime crates compile
unchanged; an ast-grep import sweep can later drop the shim. The LLSD-to-domain
converters that need sl-wire types (e.g. the inventory/material parsers)
**stay** in sl-wire.

**Binary codec** (added in the extracted crate, `sl-llsd/src/binary.rs`):
`Llsd::to_llsd_binary(&self) -> Vec<u8>` and
`parse_llsd_binary(bytes: &[u8]) -> Result<Llsd, LlsdError>`. Marker bytes
(all multi-byte integers big-endian / network order): `!` undef; `1` / `0`
boolean; `i` + 4-byte `i32`; `r` + 8-byte `f64`; `u` + 16 raw uuid bytes; `b` +
4-byte len + raw bytes (binary); `s` + 4-byte len + UTF-8 (string); `l` + 4-byte
len + UTF-8 (uri); `d` + 8-byte `f64` epoch-seconds (date); `[` + 4-byte count +
values + `]` (array); `{` + 4-byte count + count×(`k` + 4-byte len + UTF-8 key +
value) + `}` (map). Two wrinkles: (1) our `Llsd::Date` holds an ISO-8601
*string* but binary date is an `f64` epoch-seconds — the codec converts both
ways (reuse the `time`-based date handling in `llsd.rs`); (2) Firestorm writes
the trailing `]` / `}` — emit them for cross-readability and tolerate them on
parse. The cache envelope (A4) is `gzip(` 4-byte BE `u32` version `5`
`++ to_llsd_binary(map) )`.

**Boundary verified against the code (anchors for B1).** `sl-wire/src/llsd.rs`
(1318 lines) is **not** all LLSD core — it interleaves the generic value model
with sl-wire-specific CAPS builders that **stay** in sl-wire. Moves to
`sl-llsd`: the `Llsd` enum (11 variants, `:18`); the pure accessors `get` /
`index` / `as_array` / `as_map` / `as_str` / `as_i32` / `as_f64` / `as_f32` /
`as_bool` / `as_uuid` / `as_binary` / `kind` (`:47`-`:165`); the `field_*` /
`require_*` field accessors (`:178`-`:408`); the XML codec `to_llsd_xml`
(`:423`, infallible) / `parse_llsd_xml` (`:519`,
`-> Result<_, roxmltree::Error>`) with `node_to_llsd` / `push_llsd_xml`; and
`push_escaped` (`:593`, today `pub(crate)` — make it `pub` in `sl-llsd`, the
GLTF notation emitter needs it). Stays in sl-wire (sl-wire-typed, `WireError` /
keys): every `build_*` CAPS request (`build_seed_request` `:609` …
`build_fetch_inventory_request` `:637` … the `build_object_media_*` trio
`:1079`-`:1113`, `build_event_queue_*` `:1259`), the response types
`AssetUploadResponse` (`:935`) / `ObjectMediaResponse` (`:1157`) /
`EventQueueResponse` (`:1230`) with their `from_llsd`, `parse_seed_response`
(`:1213`), and the private `llsd_bool` / `llsd_int` / `llsd_string` /
`llsd_perm` / `llsd_uuid` helpers (`:1013`-). The three `sl_types` keys imported
at `:12` (`InventoryFolderKey` / `InventoryKey` / `ObjectKey`) are used **only**
by these staying builders, so the moved core's `sl-types` dependency is light
(retained per the locked decision for the typed accessors a future caller may
add).

**The re-export is a real module, not a crate alias (B1).** Because the
sl-wire-specific builders keep living in `sl-wire/src/llsd.rs`, that file stays
a real `crate::llsd` module that opens with
`pub use sl_llsd::{Llsd, parse_llsd_xml, push_escaped, …}` (re-exporting the
moved core) **and** keeps defining the builders — so both `crate::llsd::Llsd`
and `crate::llsd::build_seed_request` keep resolving at the **20** sl-wire
modules (verified count) and the downstream `sl-proto` (4 files) /
`sl-client-tokio` (7) / `sl-client-bevy` (7) call sites, all unchanged. A bare
`pub use sl_llsd as llsd` would leave the builders homeless.

**`WireError` coupling → `LlsdError` (B1).** The `field_*` / `require_*`
accessors return `Result<_, WireError>` today via two variants only —
`WireError::MalformedField { field: &'static str, value: String }`
(`error.rs:83`) and `WireError::MissingField { field: &'static str }`
(`error.rs:95`). The orphan rule forbids leaving `impl Llsd` in sl-wire once
`Llsd` is foreign, so these accessors **move** and re-type to a crate-local
`LlsdError` mirroring those two variants; `impl From<LlsdError> for WireError`
maps them back, and the `?` at every staying-builder call site
(`AssetUploadResponse::from_llsd` etc.) converts transparently. `parse_llsd_xml`
keeps its `roxmltree::Error`, so it moves clean.

**Notation reader is GLTF-entangled (B1).** `material/gltf.rs` mixes a generic
notation-LLSD cursor (`:59`-~`:300`: string/int/array token readers, "advance
past one value") with GLTF-domain decode (`modify_material_update` `:365`,
`-> Result<_, WireError>`, `GltfMaterialOverride`). B1 moves **only** the
generic cursor primitives to `sl-llsd`; the GLTF-typed decode stays in sl-wire.
If the cursor proves too entangled with GLTF byte-span semantics, B1 may keep it
in sl-wire — the A3 deliverable (the *binary* codec) is independent of it.

**Binary codec confirmed against Firestorm (anchors for B2).** Ground-truthed in
`indra/llcommon/llsdserialize.cpp` (`LLSDBinaryFormatter::format_impl` `:1541`,
`LLSDBinaryParser::doParse` `:952` / `parseMap` `:1186` / `parseArray` `:1240`)
and `newview/llinventorymodel.cpp` (`saveToFile` `:3779`, `loadFromFile`
`:3661`, `sCurrentInvCacheVersion = 5` `:97`). Tags are exactly as A3 states.
Newly pinned wrinkles B2 must honour:

- **Closing `]` / `}` are mandatory, not decorative.** `parseArray` / `parseMap`
  return `PARSE_FAILURE` if the terminator is absent — so emit them **and**
  require them on parse. The 4-byte BE count prefix is authoritative: parse
  exactly `count` entries then expect the terminator (a mismatch is an error).
- **Date endianness is asymmetric in Firestorm.** `format_impl` writes `Real`
  through `ll_htond` (network/BE) but writes `Date`'s `f64` *raw* with no swap
  (host-endian), read back raw — so Firestorm dates are host-endian, unlike
  every other multi-byte field. **But inventory caches never hit this:** item
  creation dates serialise as LLSD `Integer`
  (`LLSD::Integer(item->getCreationDate())`), not LLSD `Date`, so the cache map
  carries no `Date`. B2 still matches Firestorm for general round-trip
  (host-endian date, or document the divergence); the agent/library cache is
  unaffected either way.
- **Our `Llsd::Date` holds an ISO-8601 *string*** (`:33`, verbatim; the XML
  codec does no `time` parsing) while binary date is `f64` epoch-seconds — so
  the binary date path is the one place needing `time` (ISO ↔ epoch). `time` is
  therefore a **B2** dependency, not B1.
- **Parser tolerates notation-style strings.** `doParse` / `parseMap` also
  accept `'` / `"`-delimited string values and quoted map keys as a fallback;
  our parser tolerates them on read but only ever **emits** the length-prefixed
  `s` / `k` forms.
- **File framing (A4 cross-ref).** `saveToFile` writes `htonl(5)` (4-byte BE),
  then one binary-LLSD map `{ "categories": [...], "items": [...] }` via
  `LLSDOStreamer`, **then a trailing `\n`** (`<< std::endl`), then gzips the
  whole temp file separately (`gzip_file`). So the gzip envelope wraps
  header+map+newline; our writer appends the `\n` (harmless) and our reader
  tolerates trailing bytes after the top-level map. `loadFromFile` reads the
  4-byte version first and treats any value `!= 5` as obsolete (ignored). The
  `getVersion() != VERSION_UNKNOWN` save filter is the Firestorm anchor for
  A10's "`Loaded` folders only" snapshot.

### Disk-cache layout & directories reference (from A4)

Files `<agent-uuid>.inv.llsd.gz` (agent) and `<agent-uuid>.lib.inv.llsd.gz`
(library) are written **directly** under the caller's `agent_cache_dir` (no
derived subdir). **Crash-safe atomic write** (a save interrupted mid-write must
never corrupt or lose the existing cache): write the complete gzip to a
distinctly-named temp file **in the same directory** (e.g.
`<agent-uuid>.inv.llsd.gz.<pid>.tmp`, so it shares the target's filesystem and a
concurrent save cannot clobber it), `flush` + `fsync` it, then atomically
`rename` it over the target — POSIX `rename(2)` is atomic, so any reader or a
crash sees either the intact old file or the intact new one, never a truncated
blend; on Windows the runtime shell uses the replace-style rename. On error the
temp file is removed and the old cache is left untouched. The library cache
(`.lib.inv.llsd.gz`) is written the same way.
Load: gunzip, read the 4-byte BE version, treat the file as cold (ignore) unless
it equals `5`, else `parse_llsd_binary` the remainder. `ClientDirectories` lives
in `sl-proto` next to `ChatLogConfig` with three `Option<PathBuf>` fields —
`agent_cache_dir`, `agent_chat_log_dir`, `shared_cache_dir` (reserved) — each
`None` disabling that feature; it is passed once at each runtime's construction.
Chat-log retrofit: `ChatLog::new` takes `agent_chat_log_dir` verbatim (drop the
`.join(clean_file_name(own_name))`); the now-redundant `ChatLogConfig.log_dir`
is removed. The chat-log tests that assert the `Me Resident` subdir change to
assert files directly under the supplied dir.

**Surface verified against the code (anchors for B9/B10).** `ChatLogConfig` is
`sl-proto/src/chat_log.rs:168-202`; the field to remove is
`log_dir: Option<std::path::PathBuf>` (`:175`, defaulted `None` at `:208`), and
`clean_file_name` is `:246`. The directory is **read in exactly two places** —
the byte-identical `ChatLog::new(config, own_name, own_id)` shells at
`sl-client-tokio/src/chat_log.rs:157` **and** the bevy copy at the same `:157` —
both `config.log_dir`-or-`chat_logs/` then `.join(clean_file_name(…))`
(`:158-162`). So the retrofit drops **both** the `chat_logs/` default **and**
the `clean_file_name` join, taking `agent_chat_log_dir` verbatim (its `None`
disabling the feature — there is no longer a built-in default dir; the `enabled`
set still gates as before). `log_dir` is **set in exactly one place**:
`sl-repl/src/chat_log_args.rs:75` (`log_dir: self.chat_log_dir.clone()`) from
the `--chat-log-dir` CLI arg (`:35`); after removal `ChatLogArgs::to_config`
drops that line and the dir flows via `ClientDirectories.agent_chat_log_dir`
instead. Constructor threading sites: tokio holds
`chat_log_config: ChatLogConfig` (`sl-client-tokio/src/lib.rs:175`, set via
`set_chat_log_config` `:279`) and calls `ChatLog::new` in `run()` (`:314-318`);
bevy's plugin field is `chat_log_config` (`sl-client-bevy/src/lib.rs:142`),
calling `ChatLog::new` in `advance_login()` (`:404-408`); the REPL wires it with
`client.set_chat_log_config(args.chat_log.to_config())`
(`sl-repl-tokio/src/bin/sl-repl-tokio.rs:559`). `ClientDirectories` does **not**
exist yet (grep-confirmed); it is threaded in alongside these sites at parity.
The `Me Resident` subdir is asserted in **both** runtimes (the two chat_log.rs
are identical): tokio at `:634-645` / `:682-704` / `:741-758`
(`dir.join("Me Resident").join(...)`) and the mirrored bevy copy, each seeded by
a helper that passes `log_dir: Some(dir)` (`:619`) — B9 retypes the helper to
the verbatim dir and drops the `Me Resident` join from every assertion in
**both** crates. B10 anchors: there is **no** `flate2`/gzip dependency anywhere
yet, **no** `tokio::fs` usage, and **no** atomic temp+rename pattern — the chat
writer appends synchronously via `fs_err` + `OpenOptions`
(`sl-client-tokio/src/chat_log.rs:41-49`), so the crash-safe gzip write is
wholly new code B10 adds.

### Cache load / merge reference (from A5)

Pure (sl-proto, no I/O): a `merge_skeleton(cached, skeleton)` taking the loaded
`Inventory` plus the skeleton `&[InventoryFolder]` and returning the merged
`Inventory` and the `Vec<InventoryFolderKey>` of folders needing fetch —
mirroring Firestorm `loadSkeleton`. Start from `cached`; for each skeleton
folder `s`: if a cached entry exists with the same version, keep its contents
and set `Loaded`; else (absent or version differs) insert/keep the folder
`Unknown` and drop any cached child items/folders beneath it. Drop cached
**agent** folders absent from the skeleton (server-deleted); the **library**
subtree is not in the skeleton and is handled per-owner. Items survive only
under a folder that stayed `Loaded`. Return the merged model and the list of
`Unknown` folders (the initial fetch queue, A6). The runtime shell (A4) loads
the disk file and hands it in as `cached`.

**Algorithm verified against Firestorm + the code (anchors for B5).** The
skeleton `merge_skeleton` consumes is exactly the `Vec<InventoryFolder>` the
login arm already builds via `skeleton_folder` (`conversions.rs:951`, copying
the authoritative `folder.version` at `:957`): the **agent** skeleton at
`methods.rs:1216-1226` (also `Event::InventorySkeleton`) and the **library**
skeleton at `:1207-1213` (`Event::LibraryInventory`). So merge runs **once per
owner** — agent skeleton against the agent cache, library skeleton against the
library cache — mirroring Firestorm, which calls `loadSkeleton(options,
owner_id)` once for the agent and once for `getLibraryOwnerID()`
(`llinventorymodel.cpp:2886`), each with its own cache file. No merge function
exists yet (grep-confirmed) — B5 is net-new.

Per-folder rule confirmed against `loadSkeleton` (`:3025-3171`), with
Firestorm's `VERSION_UNKNOWN = -1` (`llviewerinventory.h:208`) ⇒ our
`FolderState::Unknown`: (1) a cached folder present in the skeleton with an
**equal** version is kept `Loaded` (added to `cached_ids`, `:3055-3070`); (2)
version **differs** ⇒ `setVersion(NO_VERSION)` ⇒ `Unknown`/refetch
(`:3055-3061`); (3) a skeleton folder **absent from cache** is added `Unknown`
(`:3076-3088`); (4) a cached folder **absent from the skeleton** is dropped
("removed from inventory", `:3049-3054`); (5) items survive **only** under a
parent still `version != NO_VERSION`, i.e. `Loaded` (`:3106`) — exactly the
reference's "items kept only under a `Loaded` folder".

Version-validity gate (A4 cross-ref): `loadFromFile` (`:3661`) yields the cached
model only when the 4-byte BE header **equals** `5` (`sCurrentInvCacheVersion`,
`:3694`) **and** the binary parse succeeds; on either failure
`is_cache_obsolete` stays true and `loadSkeleton` takes the `else` branch
(`:3162-3171`) marking **every** skeleton folder `Unknown` — a full refetch. So
B5's `inventory_from_cache_bytes` returns the same "treat as no cache" outcome
on a header/parse mismatch, and `merge_skeleton` against an empty cache yields
every skeleton folder `Unknown`.

One wrinkle B5 may fold or skip: Firestorm force-invalidates a folder whose
cached item set is suspect — an item loaded with asset type `AT_UNKNOWN` adds
its parent to `cats_to_update` (`loadFromFile:3751-3753`), which forces that
folder `NO_VERSION` (`:3043-3046`). It is a corruption-recovery guard that only
ever **adds** to the refetch set (never keeps a stale folder), so B5 may either
mirror it (invalidate a folder whose items fail to decode) or rely on the
converters rejecting malformed items earlier; either is sound.

### Background-fetch reference (from A6)

Sans-IO scheduler on `Session`: `next_inventory_fetch_batch(&mut self,
max_in_flight) -> Vec<InventoryFolderKey>` — BFS from each root over `Unknown`
folders up to the in-flight bound, flipping the returned folders to `Fetching`.
A descendents reply flips its target to `Loaded { version }` and seeds the new
children `Unknown`. The runtime shell calls the scheduler after merge and after
each reply, issuing the existing `FetchInventoryDescendents2` POST (agent) /
`FetchLibDescendents2` (library, keyed by owner). On-demand:
`request_folder_contents(folder)` enqueues exactly an `Unknown` folder.
Completion: `inventory_fully_loaded(owner)` is true when no folder under that
owner is `Unknown` / `Fetching`.

**Opt-out (user requirement).** The automatic crawl is gated by a sans-IO flag
`Session.background_inventory_fetch: bool` (setter
`set_background_inventory_fetch(&mut self, bool)`), **default `false`**: while
off, `next_inventory_fetch_batch` returns empty and the model never
auto-enqueues — so a consumer that never reads inventory (the `sl-survey`
binary, which match-ignores `Event::InventorySkeleton` / `LibraryInventory` /
`InventoryDescendents` / … at `sl-survey/src/bin/sl-survey.rs:627-700`) pays
nothing: no skeleton-triggered batch, no CAPS POSTs. The flag gates **only** the
automatic BFS; the explicit pull paths — `request_folder_contents(folder)`
(on-demand single folder) and `Command::FetchInventoryFolders(..)` — always
work regardless, so a caller can still fetch a specific folder without turning
the crawler on. The runtime shells expose it in their inventory config at parity
(the full clients / REPL / `inventory_cache` example enable it; `sl-survey`
leaves it off — the cheap default). Default-off mirrors the locked
"caller-supplies-everything / opt-in" philosophy (cache via `Option<PathBuf>`);
B6 may flip the default to on if Firestorm-parity (the viewer always
background-fetches) is preferred — the gate and threading are identical either
way.

**Existing fetch path verified against the code (anchors for B6).** Background
fetch is **net-new**: there is **no** in-flight tracking, fetch queue, or
recursive/background logic in `sl-proto` today (grep-confirmed for `background`
/ `recursive` / `fetch_queue` / `pending`); fetching is purely on explicit
command. The pieces B6 reuses: the CAPS body builder
`build_fetch_inventory_request(owner_id, folder_ids)` (taking `Uuid` +
`&[InventoryFolderKey]`,
`sl-wire/src/llsd.rs:637`, hard-codes `fetch_folders=1` / `fetch_items=1` /
`sort_order=0`); the two existing fetch commands
`Command::RequestFolderContents(InventoryFolderKey)` (`command.rs:168`, UDP) and
`Command::FetchInventoryFolders(Vec<InventoryFolderKey>)` (`command.rs:172`,
CAPS batch); the sans-IO method `request_folder_contents` (`methods.rs:7833`)
→ `circuit.send_fetch_inventory_descendents` (`circuit.rs:2779`,
fire-and-forget, matched only by `folder_id` in the reply — no callback id);
the runtime CAPS
POSTs `fetch_inventory` (`sl-client-tokio/src/inventory.rs:13`) /
`run_inventory_fetch` (`sl-client-bevy/src/inventory.rs:16`), dispatched at
tokio `lib.rs:512`/`:515` and bevy `lib.rs:642`/`:645`. Replies fold through
`cache_inventory` at the CAPS arm (`methods.rs:403`) and the UDP arm
(`methods.rs:2406`), both emitting `Event::InventoryDescendents { folder_id,
version, descendents, folders, items }` (`event.rs:469`) — so B6's
`Unknown → Fetching → Loaded { version }` flip hangs off these two existing fold
sites (the `version` from `reply.agent_data.version`), keyed by `folder_id`.
The library path is keyed by `library_owner` (`avatar_profile.rs:353`,
`Option<AgentKey>`) over `FetchLibDescendents2` (B7) reusing the same scheduler.

**Throttle/queue shape confirmed against Firestorm (anchors for B6).**
`LLInventoryModelBackgroundFetch`
(`indra/newview/llinventorymodelbackgroundfetch.{cpp,h}`). Bounded-in-flight:
legacy HTTP `max_concurrent_fetches = 12` + `max_batch_size = 10`
(`.cpp:1236-1237`); AIS v3 `max_concurrent_fetches = clamp(PoolSizeAIS - 1, 1,
50)` (default pool 20 ⇒ 19, `.cpp:913-916`) with per-request `BatchSizeAIS3`
(default 20, clamped 1–40, `.cpp:1050-1051`). B6 takes one `max_in_flight`
bound (a small constant, ~10–16) — both numbers above are settings-driven, so a
fixed sensible default suffices. BFS queue: dual deques `mFetchFolderQueue` /
`mFetchItemQueue` (`.h:138-139`) of `FetchQueueInfo` (`.h:97-108`); `bulkFetch`
(`.cpp:1220`) pops folders breadth-first and enqueues their child folders
(`.cpp:1337-1341`) — our equivalent is "seed new children `Unknown`, BFS picks
them next round". In-flight counter `mFetchCount` (`.h:132`), incremented on
dispatch / decremented on reply (`incrFetchCount` `.cpp:718`), with
`isBulkFetchProcessingComplete()` = queues empty **and** `mFetchCount <= 0`
(`.cpp:211-214`) — our `inventory_fully_loaded(owner)` (no `Unknown` /
`Fetching` under that owner). Completion signal `isEverythingFetched()` (`.h:64`
→
`mAllRecursiveFoldersFetched`, set in `setAllFoldersFetched` `.cpp:509-529`).
On-demand `start(cat, recursive)` (`.h:52`) / `scheduleFolderFetch(cat, forced)`
(`.h:53`, a `forced` fetch jumps the queue front) — our on-demand
`request_folder_contents(folder)` enqueues exactly that `Unknown` folder.
AIS-vs-legacy split in `backgroundFetch()` (`.cpp:548-558`: AIS if available
else legacy HTTP); caps `FetchInventoryDescendents2` (`.cpp:1393`) and
`FetchLibDescendents2` (`.cpp:1404`) — the same two caps our runtime already
POSTs to. We have no AIS-batch-subset path, so B6 issues one folder per
`FetchInventoryDescendents2` request (or the existing
`FetchInventoryFolders(Vec<_>)` batch), simpler than Firestorm's subset
batching but the same BFS + bounded-in-flight shape.

### Public model API reference (from A7) — idiomatic read + write

View types in `sl-proto/src/types/inventory.rs`: a borrowed
`enum Child<'a> { Folder(&'a InventoryFolder), Item(&'a InventoryItem) }`;
owning snapshots `FolderInfo` (typed `folder_id` / `parent_id`, `name`, a
resolved `FolderType`, `version`, `FolderState`) and `ItemInfo` (typed `item_id`
/ `folder_id`, `name`, `asset_id`, resolved `AssetType` / `InventoryType`, an
`Option<(SaleType, LindenAmount)>` sale, `Permissions5`, dates/creator/owner) —
typed keys + resolved enums, never raw `i8` / `u8`. An opaque
`InventoryCursor(usize)`. Borrowed accessors on `Session`: `inventory_root()` /
`library_root()`; `inventory_folder(key)` / `inventory_item(key)`;
`inventory_children(folder)` returning `impl Iterator<Item = Child<'_>>`;
`folder_fetch_state(folder)`; and a paged
`inventory_folder_page(folder, before, limit)` returning
`(Vec<FolderInfo>, Vec<ItemInfo>, Option<InventoryCursor>)` (the `history_page`
precedent). The raw `inventory_folders()` / `inventory_items()` accessors become
`#[deprecated]`.

**Verified against the code (anchors for B4).** Four A7 accessors **already
exist** on `Session` (added under #30, the inventory cache):
`inventory_root() -> Option<InventoryFolderKey>` (`methods.rs:7812`),
`inventory_folder(InventoryFolderKey) -> Option<&InventoryFolder>` (`:7848`),
`inventory_item(InventoryKey) -> Option<&InventoryItem>` (`:7855`), and
`inventory_children(InventoryFolderKey) -> (Vec<&InventoryFolder>,
Vec<&InventoryItem>)` (`:7876`). So B4 **refines** rather than building from
scratch: keep the two by-key lookups verbatim and **change**
`inventory_children`'s return from the `Vec` tuple to the `Vec`-free
`impl Iterator<Item = Child<'_>>`. That signature change is clean —
grep-confirmed there are **no external callers** of `inventory_children` /
`inventory_folders()` / `inventory_items()` anywhere in the workspace (the
runtimes call only the mutation `remove_inventory_*`: tokio `lib.rs:538`/`:556`,
bevy `:688`/`:724`). The two raw map accessors to `#[deprecated]` are
`inventory_folders() -> &BTreeMap<Uuid, InventoryFolder>` (`:7861`) and
`inventory_items() -> &BTreeMap<Uuid, InventoryItem>` (`:7867`).

**`library_root` is not on `Session` yet.** The library root from the login
response is folded into `LoginAccount.library_root: Option<InventoryFolderKey>`
(`avatar_profile.rs:350`, set at `methods.rs:1202`) and reachable only via
`login_account().library_root` (`:7820`). B4 adds `Session::library_root() ->
Option<InventoryFolderKey>` reading from the stored login account — no new state
(A9/B7 later holds the library *tree*; A7's accessor only needs the root id).

**Resolved-enum converters verified (anchors for B4).** `ItemInfo` resolves its
three raw `i8` / `u8` fields with existing converters, all `#[non_exhaustive]`
enums with a fallback arm already exercised elsewhere (`chat.rs:459`,
`methods.rs:2649`): `AssetType::from_code(i32)` (`asset.rs:102`) on
`i32::from(item.item_type)`; `InventoryType::from_code(i32)` (`asset.rs:286`) on
`i32::from(item.inv_type)`; `SaleType::from_code(u8)` (`editing.rs:169`) on
`item.sale_type`. The sale field pairs that `SaleType` with the existing
`InventoryItem.sale_price: Option<LindenAmount>` (already `None` when not for
sale) into `Option<(SaleType, LindenAmount)>`. `Permissions5`, `creation_date`,
and `creator_id` / `owner` / `group` copy across unchanged (already typed).

**`FolderType` does not exist — B4 must add it.** `FolderInfo`'s resolved
`folder_type` has **no** converter today (grep-confirmed: `FolderType` appears
only in doc comments). B4 adds a `FolderType` enum to `sl-proto` modelled on LL
`LLFolderType::EType` (`indra/llinventory/llfoldertype.h:39-108`). It is **not**
`AssetType`: folder preferred-types add folder-only codes and one collides
(`AT_CATEGORY = 8` vs `FT_ROOT_INVENTORY = 8`), so reusing `AssetType` would
resolve wrongly. Cover at least the protected/system types — `Texture = 0` …
`Bodypart = 13` (shared with assets), plus `RootInventory = 8`, `Trash = 14`,
`LostAndFound = 16`, `Favorite = 23`, `CurrentOutfit = 46`, `Outfit = 47`,
`MyOutfits = 48`, `Inbox = 50`, `Outbox = 51`, `MarketplaceListings = 53`,
`Settings = 56`, `Material = 57` — with `None = -1` and an `Other(i8)` fallback,
plus `from_code(i8)` / `to_code() -> i8` mirroring the existing enums' pattern.

**Cursor + pagination shape.** `InventoryCursor(usize)` mirrors `MessageCursor`
(`chat_session.rs:391`) exactly: crate-private `new` / `consumed` for in-crate
paging plus `pub from_consumed` / `consumed_count` so A8's channel runtimes can
carry it across the `Command` / `Event` boundary. `inventory_folder_page(folder,
before: Option<InventoryCursor>, limit) -> (Vec<FolderInfo>, Vec<ItemInfo>,
Option<InventoryCursor>)` mirrors `history_page` (`methods.rs:5057`) for the
*cursor* shape, but returns **owning** view-type `Vec`s (not borrowed iterators)
because `FolderInfo` / `ItemInfo` are snapshots. One cursor walks the
**combined** child sequence (folders first, then items, in the deterministic
parent→children-index order) so a single page can span the folder/item boundary
of one mixed folder. Zero-copy borrowed walking stays available through
`inventory_children` (the `Child` iterator, for bevy's `&Session` reader);
`inventory_folder_page` is the owning / `Arc`-friendly read behind A8's pull
bridge.

**Ordering.** `FolderState` (the `Unknown` / `Fetching` / `Loaded { version }`
type) and the parent→children index land in **B3** (model + fetch-state), before
B4 — so `folder_fetch_state(folder) -> Option<FolderState>` and
`FolderInfo.state: FolderState` have their type ready when B4 builds the read
API on top.

**Write side — symmetric with the read side (anchors for B4).** A full mutation
surface already exists on `Session` and already does
**optimistic local cache updates** with typed keys: `create_inventory_folder`
(`methods.rs:7962`), `update_inventory_folder` (`:7995`),
`move_inventory_folder(s)` (`:8032`/`:8049`), `remove_inventory_folders`
(`:8076`), `create_inventory_item` (`:8100`), `link_inventory_item` (`:8121`),
`update_inventory_item` (`:8140`), `move_inventory_item(s)` (`:8160`/`:8178`),
`copy_inventory_item` (`:8212`), `remove_inventory_items` (`:8239`),
`change_inventory_item_flags` (`:8260`), `purge_inventory_descendents`
(`:8281`), `remove_inventory_objects` (`:8299`). The cache-coherence *mechanics*
(route every one through the model so the index / `FolderState` / authoritative
version stay consistent) are already owned by A2/B3 (the move/remove/seed sites
are enumerated there); A10 owns the `dirty` flag set by each. A7 adds only the
**idiomatic-ergonomics** layer so the write side stops being the raw twin of the
now-typed read side:

- **Typed enum params instead of raw `i8`/`u8`** (mirror the read views): folder
  create/update take `FolderType` (not `folder_type: i8`); `NewInventoryItem`
  (`inventory.rs:79`) takes `AssetType` / `InventoryType` / `WearableType` (the
  last already exists, `appearance.rs:19`) for its `asset_type` / `inv_type` /
  `wearable_type`; `NewInventoryLink` (`:121`) takes `AssetType` (`AT_LINK` /
  `AT_LINK_FOLDER`) / `InventoryType`; each `.to_code()`s for the wire builder.
  Keep `flags: u32` a bitfield (out of scope).
- **Folder-id allocation stays caller-supplied** — `sl-proto` is sans-IO and
  generates **no** UUIDs (grep-confirmed: no `new_v4` / `rand` / `getrandom`),
  so the runtime shell mints the fresh v4 id, not `Session`. The protocol
  asymmetry is inherent (client allocates *folder* ids, the sim allocates *item*
  ids and echoes a callback id, `:8104`/`:8126`) — document it rather than hide
  it. The footgun fix is **validation, not generation**:
  `create_inventory_folder` returns an `Error` for a nil or already-present
  `folder_id` instead of silently clobbering the cache, and returns the new
  `InventoryFolderKey` for symmetry with the read accessors.
- **Local guards off the B3 index (O(1)):** `move_inventory_folders` rejects a
  move whose target is the folder itself or one of its descendants (a cycle) and
  a move to a parent not in the model — surfaced as `Error` before the wire
  send, not silent corruption (the in-place re-parent at `:8062` currently
  trusts the caller).
- **Focused, clobber-free updates:** `update_inventory_folder` (`:7995`) /
  `update_inventory_item` (`:8140`) are all-fields `UpdateInventory*` overwrites
  (the wire shape) — a caller editing one attribute can accidentally re-parent
  or reset permissions/owner. Add convenience wrappers —
  `rename_inventory_folder` / `rename_inventory_item` (and a re-type /
  set-permissions equivalent) — that read the untouched fields from the cached
  folder/item (now reachable via the read model) and submit the full message, so
  single-attribute edits can't clobber the rest. The raw all-fields methods stay
  for power users.
- **Optimistic → authoritative reconciliation policy (B3/A10 mechanics, named
  here):** an optimistic edit updates the model + sets `dirty` immediately with
  a *guessed* version (create uses `1`, `:7983`); the folder stays `Loaded`
  (contents are known). When the authoritative reply folds in —
  `BulkUpdateInventory` (`methods.rs:2467` → `Event::InventoryBulkUpdate`),
  `UpdateCreateInventoryItem` (`:2432` → `Event::InventoryItemCreated`, carrying
  the sim-allocated item id correlated by the `InventoryCallbackId`), or a later
  descendents refetch — it **overwrites** the optimistic guess (server is
  authoritative, last-write-wins), and `cache_inventory_folder`'s existing
  preserve-on-`version 0` guard (`:7898-7903`) keeps a real version from being
  clobbered by a child fold.

### Pull-bridge & runtime-divergence reference (from A8)

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

### Library-inventory reference (from A9)

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

### Persistence & lifecycle reference (from A10)

The model is grid-level: add **no** inventory clear at the four region-boundary
sites (`begin_handover`, `promote_child_to_root`, `TeleportLocal`, child
`DisableSimulator`) — the chat persistence-guard precedent (CHAT_ROADMAP B10).
Save points: on logout and on a dirty/idle tick. `Inventory.dirty` is set by
every fold / mutation and cleared on save. The cacheable snapshot collects only
`Loaded` folders + their items (skip `Unknown` / `Fetching`), matching Firestorm
`LLCanCache`. sans-IO surface: a `cacheable_cache_bytes(owner)` builder (A4
bytes) and a `load_cached(owner, Inventory)` intake; the runtime shell owns the
file / gzip I/O and the save timing.

### Test & verification reference (from A11)

Extend `sl-proto/tests/lifecycle.rs` (reuse `established` + the inbound
builders): skeleton seeds roots + `Unknown`; a descendents reply flips the
folder `Loaded` and the index lists its children; binary-LLSD round-trip (each
variant + the cache map, cross-checked against the XML path); `merge_skeleton`
keeps a version-matching folder (absent from the returned fetch list) and drops
a stale one (present in it); a pagination cursor walks a large folder; inventory
**survives teleport** (mirror the chat persistence test — seed a `Loaded` tree,
drive the handover, assert intact); the library tree sits under its own root.
Runtime (tokio): a caller temp dir round-trips save → gunzip → 4-byte header `5`
→ load → `merge_skeleton` → model equality, plus the chat-log verbatim-dir
change. Live (OpenSim, second test avatar): first login fetches + writes the
cache; second login loads it and skips refetch of version-matching folders
(observed via diagnostics).

## Phase B tasks — consolidated (B1–B11)

Derived from the signed-off § Phase A design references and dependency-ordered
so each task leaves the tree buildable, clippy-clean (restriction lints), and
`cargo test`-green on its own (sl-proto's `[lints.rust]` denies the `unused_*`
family and the ggh pre-commit re-runs full clippy every attempt, so every
field/type lands **with** its writer, its reader, and its tests). Each task
names the reference it implements. Keep `sl-client-tokio`, `sl-client-bevy`, and
the REPL at parity; never push client-only types into shared `sl-types`.
**Ask the user before starting Phase B** (the standing "ask before new roadmap
work" rule).

### B1. Extract the `sl-llsd` crate (from A3)

Fully standalone (no inventory dependency); first because every later task
serialises or parses LLSD and B2 adds the binary codec here.

- [ ] Add a new `sl-llsd` workspace member; move **only the core** (per the A3
      boundary): the `Llsd` enum (`:18`), the pure accessors (`get`/`index`/
      `as_*`/`kind`, `:47`-`:165`), the `field_*` / `require_*` accessors
      (`:178`-`:408`), the XML codec (`to_llsd_xml` `:423` / `parse_llsd_xml`
      `:519`, with `node_to_llsd` / `push_llsd_xml`), `push_escaped` (`:593`,
      make it `pub`), and the generic notation cursor from
      `sl-wire/src/material/gltf.rs` (`:59`-~`:300`). Dependencies: `sl-types`,
      `uuid`, `base64`, `roxmltree` (`time` lands with B2, the binary date
      path). The `build_*` CAPS builders, the `AssetUploadResponse` /
      `ObjectMediaResponse` / `EventQueueResponse` types, the `llsd_*` helpers,
      and the GLTF-domain decode (`modify_material_update`) **stay** in sl-wire.
- [ ] Introduce a crate-local `LlsdError` mirroring the two `WireError` variants
      the moved accessors use —
      `MalformedField { field: &'static str, value: String }` (`error.rs:83`)
      and `MissingField { field: &'static str }` (`error.rs:95`) — re-type the
      moved `field_*` / `require_*` to it, and add
      `impl From<LlsdError> for WireError` in sl-wire so the `?` at every
      staying builder (`AssetUploadResponse::from_llsd` etc.) still converts.
- [ ] Keep sl-wire compiling: `sl-wire/src/llsd.rs`
      **stays a real `crate::llsd` module** — it opens with
      `pub use sl_llsd::{Llsd, parse_llsd_xml, push_escaped, …}` (re-export the
      moved core) **and** keeps the builders, so both `crate::llsd::Llsd` and
      `crate::llsd::build_seed_request` resolve at the 20 sl-wire modules +
      downstream `sl-proto` (4) / `sl-client-tokio` (7) / `sl-client-bevy` (7)
      call sites unchanged. (A bare `pub use sl_llsd as llsd` would leave the
      builders homeless.)
- [ ] Verify: full workspace builds + `cargo test` green, clippy-clean. Split
      the tests by where their subject landed: the pure-LLSD cases in
      `sl-wire/tests/llsd.rs` + the inline `field_accessors_*` test (`:1285`)
      move to `sl-llsd`; the builder/CAPS cases (`AssetUploadResponse`,
      `EventQueue`, `ObjectMedia`) stay in sl-wire.

### B2. Binary-LLSD codec in `sl-llsd` (from A3)

Standalone; the cache tasks (B5/B10) serialise through it.

- [ ] Add `sl-llsd/src/binary.rs` (+ the `time` dep, for the date path):
      `Llsd::to_llsd_binary(&self) -> Vec<u8>` and
      `parse_llsd_binary(bytes: &[u8]) -> Result<Llsd, LlsdError>` over all 11
      `Llsd` variants, per the A3 tag-byte spec; export it. Honour the A3-pinned
      Firestorm wrinkles: **emit and require** the closing `]` / `}` (a missing
      terminator is an `Err`), treat the 4-byte BE count as authoritative,
      tolerate notation-style `'` / `"` strings + quoted keys on read but only
      emit length-prefixed `s` / `k`, and convert `Llsd::Date` (ISO-8601 string)
      ↔ `f64` epoch-seconds — matching Firestorm's host-endian raw `Date` write
      (`Real` stays BE via `ll_htond`).
- [ ] Round-trip tests: each variant individually; a nested map/array; the cache
  map shape `{ categories: [...], items: [...] }` (note item creation dates are
  LLSD `Integer`, not `Date`, so the cache map never exercises the date path);
  and `binary → Llsd` equals `xml → Llsd` for a shared fixture (cross-check
  against the existing XML path).
- [ ] A decode-robustness test (truncated / bad-tag / missing-terminator /
  count-mismatch input ⇒ `Err`, no panic; no indexing-panic — restriction-lint
  clean).

### B3. Held `Inventory` model + fetch-state + index (from A1·A2)

Folds the existing raw maps into a model module; migrates the `cache_inventory*`
folds and the accessors onto it with no behaviour change yet.

- [ ] Add `sl-proto/src/session/inventory.rs`: an `Inventory` struct owning the
      folder/item stores, the `owner` discriminator (`Agent` / `Library`), the
      roots, a `FolderState` (`Unknown` / `Fetching` /
      `Loaded { version: i32 }`) per folder, and the parent→children index.
      `const fn` empty constructor.
- [ ] Move `inventory_folders` / `inventory_items` / `inventory_root` /
  `next_inventory_callback` off `Session` into the model field; migrate every
  internal use to maintain the index + fetch-state: the central folds
  (`cache_inventory`/`_folder`/`_item`, `:7910`/`:7897`/`:7920`), the **direct
  skeleton seed** (`:1223-1224` — route through the model so it sets the
  authoritative version + index instead of a raw `insert`), the **re-parent
  mutations** that edit the parent link in place (`move_inventory_folders`
  `:8062`, `move_inventory_items` `:8193` — unlink-old + link-new), and the
  removal sites (`purge_cached_descendents` `:7926`, `remove_inventory_folders`
  `:8085`, `remove_inventory_items` `:8248`, `purge_inventory_descendents`
  `:8288`, `remove_inventory_objects` `:8310`).
- [ ] Set the authoritative folder version from the skeleton (`methods.rs:1223`)
  and from descendents replies; keep sub-folders (`version 0`) `Unknown`.
- [ ] **Persistence guard (from A10):** add **no** inventory clear at the four
  region-boundary sites; assert that in B11.
- [ ] Tests: skeleton seeds roots + `Unknown`; a descendents reply flips the
  folder to `Loaded { version }` and the index lists its children; a mutation
  keeps the index consistent.

### B4. Idiomatic public model API — read + write (from A7)

*Read side:*

- [ ] Add the `Child` enum + `FolderInfo` / `ItemInfo` view types (typed keys,
  resolved `FolderType` / `AssetType` / `InventoryType` / `SaleType` enums) in
  `sl-proto/src/types/inventory.rs`; add the **new** `FolderType` enum (LL
  `LLFolderType::EType`, folder-only codes — **not** `AssetType`, which collides
  at `8`) and the opaque `InventoryCursor` (mirror `MessageCursor`).
- [ ] On `Session`: keep the already-present `inventory_folder` /
      `inventory_item` by-key lookups (`methods.rs:7848` / `:7855`) verbatim;
      **change** `inventory_children(folder)` (`:7876`) from the `Vec` tuple to
      `impl Iterator<Item = Child<'_>>` (no external callers); add the **new**
      `library_root()` (from `login_account().library_root`),
      `folder_fetch_state(folder) -> Option<FolderState>` (B3's type), and a
      paged `inventory_folder_page(folder, before, limit)` returning the owning
      view-type window + next `InventoryCursor` (the `history_page` cursor
      precedent — owning `Vec`s, not borrowed iterators).
- [ ] Deprecate (`#[deprecated]`) the raw `inventory_folders()` (`:7861`) /
  `inventory_items()` (`:7867`) map accessors; keep them compiling one cycle.

*Write side (symmetric with the read side — coherence mechanics stay in
B3/A10):*

- [ ] Re-type the mutation params from raw `i8`/`u8` to the resolved enums:
      folder create/update (`methods.rs:7962`/`:7995`) take `FolderType`;
      `NewInventoryItem` (`inventory.rs:79`) takes `AssetType` / `InventoryType`
      / `WearableType` (reuse the existing `appearance.rs:19` enum);
      `NewInventoryLink` (`:121`) takes `AssetType` / `InventoryType`;
      `.to_code()` at the wire builder. Propagate the same enum typing to the
      matching write `Command` variants so tokio / bevy / REPL stay at parity.
- [ ] Make `create_inventory_folder` (`:7962`) return the new
      `InventoryFolderKey` and **error** on a nil or already-present id (the
      caller still mints the v4 id — `sl-proto` is sans-IO, no UUID generation);
      document the inherent client-folder-id / sim-item-id asymmetry.
- [ ] Add cycle / unknown-parent guards to `move_inventory_folders` (`:8049`,
      the in-place re-parent at `:8062`) using the B3 index — return an `Error`
      before the wire send rather than corrupting the tree.
- [ ] Add clobber-free convenience helpers — `rename_inventory_folder` /
      `rename_inventory_item` (and a re-type / set-permissions equivalent) —
      that read the untouched fields from the cached folder/item and submit the
      full `UpdateInventory*`, so a single-attribute edit can't accidentally
      re-parent or reset permissions; keep the raw all-fields
      `update_inventory_*` for power users.
- [ ] Tests: tree-walk over a seeded tree; pagination cursor across a large
      mixed folder (folders then items); view types carry typed keys + resolved
      enums (incl. `FolderType`); a write helper takes/returns the resolved
      enums; `create_inventory_folder` errors on a nil/duplicate id and returns
      the new key; a cycle-forming `move_inventory_folders` errors and leaves
      the tree unchanged; `rename_inventory_*` preserves the other fields; an
      optimistic create is overwritten by the authoritative
      `InventoryItemCreated` / `BulkUpdateInventory` fold (reconciliation).

### B5. Pure cache (de)serialise + merge/version-validity (from A3·A5)

- [ ] Add `sl-proto/src/inventory_cache.rs` (pure, no I/O / gzip / clock): an
      `inventory_to_cache_bytes(&Inventory, owner)` builder returning `Vec<u8>`
      (version header + binary LLSD via B2 + the existing `_to_llsd`
      converters), and an `inventory_from_cache_bytes(&[u8])` parser returning a
      `Result<CachedInventory, _>` (version-check, parse).
- [ ] Add the merge function `merge_skeleton(cached: Inventory, skeleton:
      &[InventoryFolder]) -> (Inventory, Vec<InventoryFolderKey>)`, run **once
      per owner** (agent skeleton `methods.rs:1216-1226` vs library skeleton
      `:1207-1213` / `Event::LibraryInventory`): version-match keeps the cached
      contents + `Loaded`; mismatch / skeleton-only ⇒ `Unknown` (dropping its
      cached children); a cached folder absent from the skeleton is dropped;
      items kept only under a folder that stayed `Loaded`. Return the `Unknown`
      set as the B6 fetch queue. (Firestorm `loadSkeleton`
      `llinventorymodel.cpp:3025-3171`, `VERSION_UNKNOWN = -1` ⇒ `Unknown`.)
- [ ] Tests: bytes round-trip to an equal model; version-header mismatch ⇒
      rejected (and merge against that empty result ⇒ every skeleton folder
      `Unknown`); merge keeps a matching folder (absent from the fetch set) and
      drops a stale one (present in the fetch set); a server-deleted folder is
      dropped; an item under a now-`Unknown` folder is dropped while one under a
      kept `Loaded` folder survives.

### B6. Background-fetch orchestration — agent tree (from A6)

- [ ] Add the sans-IO scheduler on `Session`: `next_inventory_fetch_batch(&mut
  self, max_in_flight) -> Vec<InventoryFolderKey>` (BFS over `Unknown`, bounded
  in-flight), flipping returned folders to `Fetching`; the existing descendents
  folds (CAPS `methods.rs:403` / UDP `:2406`, both via `cache_inventory`) flip
  the target to `Loaded { version }` (version from `reply.agent_data.version`)
  and seed new children `Unknown`; an on-demand query for an `Unknown` folder
  reuses the existing `request_folder_contents` (`methods.rs:7833`).
- [ ] Add the **opt-out gate** (user requirement): a sans-IO flag
  `background_inventory_fetch: bool` on `Session` with
  `set_background_inventory_fetch(&mut self, bool)`, **default `false`** — while
  off, `next_inventory_fetch_batch` returns empty and nothing auto-enqueues (so
  `sl-survey` pays nothing). The flag gates **only** the automatic BFS; the
  explicit pulls (`request_folder_contents`, `Command::FetchInventoryFolders`)
  still work when off. Expose it through the runtime inventory config at parity
  (tokio / bevy / REPL); the runtime crawl tick (after merge, after each reply)
  calls the scheduler only when enabled. Leave `sl-survey` on the default-off
  path (no enable call needed).
- [ ] A completion query (`inventory_fully_loaded(owner)` — no `Unknown` /
      `Fetching` under that owner), and a re-arm on cache merge (the
      `folders_needing_fetch` set from B5 seeds the queue).
- [ ] Tests: a merged tree with N `Unknown` folders drains over bounded batches
      to fully `Loaded`; an on-demand `Unknown` query schedules exactly that
      folder; with the gate **off**, `next_inventory_fetch_batch` returns empty
      even with `Unknown` folders present while `request_folder_contents` still
      schedules its one folder.

### B7. Library inventory — hold + fetch + separate cache (from A9)

- [ ] Capture the library root + owner into the model: `library_root`
      (`Option<InventoryFolderKey>`) and `library_owner` (already
      `Option<AgentKey>`, `avatar_profile.rs:353` — widen to `OwnerKey::Agent`
      where the model wants a uniform owner). Fold the already-emitted
      `Event::LibraryInventory` (`methods.rs:1213`) skeleton into the model
      under the `InventoryOwner::Library` tag (seeding `FolderState` from the
      skeleton version like the agent path); per-owner-merge it against the
      library cache (A5). Add the `library_root()` / `library_owner()`
      accessors. **No new login parse or skeleton event** — both already land on
      master.
- [ ] Add `CAP_FETCH_LIBRARY = "FetchLibDescendents2"` in
      `sl-proto/src/session.rs` beside `CAP_FETCH_INVENTORY` (`:63`). Fetch the
      library read-only: route a library-owned folder to the library cap URL +
      `library_owner` (reusing the unchanged
      `build_fetch_inventory_request(owner_id, …)`, `llsd.rs:637`) at both
      runtime dispatch arms (tokio `lib.rs:512-526`, bevy `lib.rs:642-658`,
      which today pass `agent_id()` + the agent cap), and `LibraryAPIv3` on SL.
      Parameterize the UDP `send_fetch_inventory_descendents`
      (`circuit.rs:2779`, currently owner-hardcoded `:2790`) +
      `request_folder_contents` (`methods.rs:7833`) with an `owner_id` so the
      library fetches over UDP too (the OpenSim-testable path — OpenSim does not
      serve the library cap). Reuse B6's scheduler keyed by owner. Persist/load
      from the separate `<agent-uuid>.lib.inv.llsd.gz` (B5 bytes,
      `InventoryOwner::Library`).
- [ ] Tests: library tree held under its own root, distinct from the agent root;
  the library skeleton folds under `InventoryOwner::Library`; the library cache
  round-trips to its own file; no mutation command targets it.

### B8. `Command`/`Event` pull-bridge (from A8)

- [ ] Add `Command::QueryInventoryFolder { folder, before, limit }` +
      `Command::QueryInventoryRoots` in `sl-proto/src/command.rs`; add
      `Event::InventoryFolderPage` (carrying
      `{ folder, folders: Arc<[FolderInfo]>, items: Arc<[ItemInfo]>, prev }`)
      and `Event::InventoryRoots { agent_root, library_root }` in
      `sl-proto/src/types/event.rs`, each documented "**synthesized locally**"
      like `Event::ChatHistoryPage` (`event.rs:919-930`). **No `methods.rs`
      dispatch arm** — the bridge mirrors `QueryChatHistoryPage`, which is
      answered in the runtimes, not in sl-proto.
- [ ] Synthesize each reply in **both** runtimes' command-dispatch arms beside
      the existing `QueryChatHistoryPage` ones (tokio `lib.rs:1622-1650`, bevy
      `lib.rs:2716-2748`): call the pure `inventory_folder_page` (B4) /
      `inventory_root()` / `library_root()` read methods on `Session` and push
      the `Arc<[…]>`-payload event onto the stream. Wire the REPL query/page
      beside its chat-history wiring (`sl-repl/src/registry.rs:5240`,
      `sl-repl-tokio/src/bin/sl-repl-tokio.rs:504`/`:597-600`). A bevy reader
      system may additionally borrow `&Session` and call `inventory_folder_page`
      directly (zero-copy); the bridge arm stays for parity.
- [ ] Tests: a folder query replies with the paged view-type window + cursor; an
  `Unknown`-folder query schedules its fetch (ties to B6); the page payloads are
  `Arc<[…]>`-shaped and the direct-borrow `inventory_children` reader compiles
  against `&Session` with no cache clone (the no-unnecessary-copy budget).

### B9. `ClientDirectories` struct + chat-log retrofit (from A4)

- [ ] Add `ClientDirectories` (three `Option<PathBuf>` fields —
      `agent_cache_dir`, `agent_chat_log_dir`, `shared_cache_dir`)
      in `sl-proto` (next to `ChatLogConfig`, `chat_log.rs`); thread it through
      the constructor sites at parity — tokio's `chat_log_config` field +
      `set_chat_log_config` (`sl-client-tokio/src/lib.rs:175`, `:279`), bevy's
      plugin `chat_log_config` field (`sl-client-bevy/src/lib.rs:142`), and the
      REPL wiring (`sl-repl-tokio/src/bin/sl-repl-tokio.rs:559`).
- [ ] Retrofit **both** `ChatLog::new` shells —
      `sl-client-tokio/src/chat_log.rs:157` **and** the byte-identical
      `sl-client-bevy/src/chat_log.rs:157` — to take `agent_chat_log_dir`
      **verbatim**, dropping **both** the `chat_logs/` default and the
      `.join(clean_file_name(own_name))` (`:158-162` in each); pass the dir from
      the new `ClientDirectories` at the call sites (tokio `run()` `:314-318`,
      bevy `advance_login()` `:404-408`). Remove the now-redundant
      `ChatLogConfig.log_dir` field (`chat_log.rs:175`, defaulted `:208`) and
      drop the line that sets it in `ChatLogArgs::to_config`
      (`sl-repl/src/chat_log_args.rs:75`).
- [ ] Update the `Me Resident` subdir tests in **both** runtimes (the two
  chat_log.rs are identical): tokio `:634-645` / `:682-704` / `:741-758` and the
  mirrored bevy copy, plus the `log_dir: Some(dir)` test helper (`:619` in each)
  — assert files directly under the supplied dir (no `Me Resident` join).

### B10. Runtime cache shells (tokio + bevy) (from A4·A5·A10)

- [ ] Add an `inventory_cache.rs` runtime shell to each of `sl-client-tokio` and
      `sl-client-bevy`: locate `<agent-uuid>.inv.llsd.gz` / `.lib.inv.llsd.gz`
      **directly** under `agent_cache_dir` (`None` ⇒ caching disabled); gzip via
      new `flate2` dep (none in the workspace today); async (`tokio::fs`,
      currently unused) vs blocking I/O — the chat writer's sync
      `fs_err`+`OpenOptions` append (`sl-client-tokio/src/chat_log.rs:41-49`) is
      *not* the pattern here; this crash-safe gzip write is all-new code.
      **Crash-safe atomic write per A4:** stream the gzip to a same-directory
      `…<pid>.tmp`, flush + `fsync`, then atomic `rename` over the target (never
      overwrite the live file in place); remove the temp + keep the old cache on
      any error.
- [ ] `InventoryCacheConfig` (enable flags, library-cache toggle) beside the dir
  from `ClientDirectories`. Load at login (before/with the skeleton) → call the
  B5 merge → seed B6's fetch queue; save on logout + on the dirty/idle interval
  using the B5 cacheable snapshot (`Loaded` folders only).
- [ ] Tests: a caller-supplied temp dir round-trips save → gunzip → 4-byte
      header `5` → load → merge → model equality (and the Firestorm-shaped keys
      are present).

### B11. Cross-cutting tests + example (from A11)

- [ ] Extend `sl-proto/tests/lifecycle.rs`: inventory **survives teleport**
      (seed a `Loaded` tree + index, drive the region handover, assert all still
      present — mirror the chat persistence test); a cache-merge relogin path
      skips refetch of version-matching folders; the chat-log verbatim-dir
      behaviour.
- [ ] Update the `sl-client-tokio/examples/inventory_edit.rs` (or a new
  `inventory_cache` example) to first-login-fetch-and-write then
  second-login-load-and-skip, observable via diagnostics.
- [ ] Live verify on OpenSim (`opensim.service`, second test avatar): first
      login writes the cache; second login loads it and does not refetch
      already-current folders (confirm via the diagnostics / log).
- [ ] Gate: `cargo fmt --all`, full clippy (restriction lints), `rumdl` on this
  file (80-col), on the current branch.
