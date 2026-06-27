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
- [ ] **A3. Extract the `sl-llsd` crate & specify the binary codec.** Pull the
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
- [ ] **A4. Design the disk-cache file layout & the `ClientDirectories`
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
- [ ] **A5. Design cache load / merge-with-skeleton / version-validity.** The
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
- [ ] **A6. Design background-fetch orchestration.** A breadth-first walk over
      folders in state `Unknown`, emitting `FetchInventoryDescendents2` requests
      (agent tree) and `FetchLibDescendents2` (library tree), throttled/batched
      (a bounded number in flight, like Firestorm's background fetch), flipping
      folders `Unknown → Fetching → Loaded` as replies land. The *decision* of
      what to fetch next is sans-IO in `sl-proto` (a method returning the next
      batch of folder ids); the *I/O* (the CAPS POST) is the runtime shell,
      reusing the existing fetch path. Define the completion signal (tree fully
      `Loaded`) and the on-demand path (a user query for an `Unknown` folder
      triggers its fetch).
- [ ] **A7. Design the idiomatic public read-model API.** Borrowed, typed
      tree-walk accessors on `Session`: `inventory_root`/`library_root` (typed
      `InventoryFolderKey`), a `Vec`-free
      `inventory_children(folder) -> impl Iterator` yielding a `Child` enum
      (`Folder(&InventoryFolder)` / `Item(&InventoryItem)`) or split folder/item
      iterators, a folder/item lookup by typed key,
      `folder_fetch_state(folder)`, and snapshot **view types** (`FolderInfo` /
      `ItemInfo`, owning, `Arc`-friendly, exposing typed keys + resolved enums
      like `AssetType` / `InventoryType` / `SaleType` instead of raw `i8`/`u8`).
      Pagination for large folders via an opaque `InventoryCursor` (the
      `MessageCursor` precedent). Deprecate `inventory_folders()` /
      `inventory_items()` (raw `&BTreeMap<Uuid, …>`).
- [ ] **A8. Design the `Command`/`Event` pull-bridge & runtime divergence.** For
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
- [ ] **A9. Design Library-inventory holding & caching.** Source the library
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

### Disk-cache layout & directories reference (from A4)

Files `<agent-uuid>.inv.llsd.gz` (agent) and `<agent-uuid>.lib.inv.llsd.gz`
(library) are written **directly** under the caller's `agent_cache_dir` (no
derived subdir). Atomic write: write `<file>.tmp`, then rename over the target.
Load: gunzip, read the 4-byte BE version, treat the file as cold (ignore) unless
it equals `5`, else `parse_llsd_binary` the remainder. `ClientDirectories` lives
in `sl-proto` next to `ChatLogConfig` with three `Option<PathBuf>` fields —
`agent_cache_dir`, `agent_chat_log_dir`, `shared_cache_dir` (reserved) — each
`None` disabling that feature; it is passed once at each runtime's construction.
Chat-log retrofit: `ChatLog::new` takes `agent_chat_log_dir` verbatim (drop the
`.join(clean_file_name(own_name))`); the now-redundant `ChatLogConfig.log_dir`
is removed. The chat-log tests that assert the `Me Resident` subdir change to
assert files directly under the supplied dir.

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

### Public read-model API reference (from A7)

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

### Pull-bridge & runtime-divergence reference (from A8)

Commands `Command::QueryInventoryFolder { folder, before, limit }`,
`Command::QueryInventoryRoots`, and `Command::RequestFolderContents { folder }`
(on-demand fetch). Replies `Event::InventoryFolderPage { folder, folders:
Arc<[FolderInfo]>, items: Arc<[ItemInfo]>, prev }` and `Event::InventoryRoots {
agent_root, library_root }`. `methods.rs` answers each query from the model.
tokio / REPL use this pull bridge (`Arc<[…]>` snapshots, O(1) hand-off); bevy
reads `&Session` directly (zero-copy). Parity = identical data / commands / view
types; only the read *transport* differs. Wire every new command across the
tokio / bevy / REPL dispatch sites.

### Library-inventory reference (from A9)

The login response carries the library owner id and library root
(`inventory-lib-owner` / `inventory-lib-root`). Hold the library tree in the
same `Inventory` under `owner = Library`, root `library_root`. Fetch it
read-only over `FetchLibDescendents2` (or UDP `FetchInventoryDescendents` with
the library owner) and `LibraryAPIv3` on SL — no mutation command targets it.
Persist / load via the separate `<agent-uuid>.lib.inv.llsd.gz` (A4) with the
same version-gated validity; its folder versions are stable, so it almost always
loads fully from cache. Fetch policy: **lazy** — a library folder's contents are
fetched on first query, or in a background slot after the agent tree, given its
size.

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

- [ ] Add a new `sl-llsd` workspace member; move the `Llsd` enum, the XML codec
      (`to_llsd_xml` / `parse_llsd_xml`), the notation reader from
      `sl-wire/src/material/gltf.rs`, and the typed-key accessors into it.
      Dependencies: `sl-types`, `uuid`, `base64`, `roxmltree`, `time`.
- [ ] Introduce a crate-local `LlsdError` (replacing the
      `crate::error::WireError` uses in the moved code) and
      `impl From<LlsdError> for WireError` in sl-wire.
- [ ] Keep sl-wire compiling: `pub use sl_llsd as llsd` (+ re-export the moved
      free functions) so the 20 sl-wire modules and the downstream `sl-proto` /
      runtime crates are unchanged. The sl-wire-typed LLSD-to-domain converters
      stay in sl-wire.
- [ ] Verify: full workspace builds + `cargo test` green, clippy-clean; the
      existing LLSD tests move with the code and still pass.

### B2. Binary-LLSD codec in `sl-llsd` (from A3)

Standalone; the cache tasks (B5/B10) serialise through it.

- [ ] Add `sl-llsd/src/binary.rs`: `Llsd::to_llsd_binary(&self) -> Vec<u8>`
      and `parse_llsd_binary(bytes: &[u8]) -> Result<Llsd, LlsdError>`
      over all 11 `Llsd` variants, per the A3 tag-byte spec; export it.
- [ ] Round-trip tests: each variant individually; a nested map/array; the cache
  map shape `{ categories: [...], items: [...] }`; and `binary → Llsd` equals
  `xml → Llsd` for a shared fixture (cross-check against the existing XML path).
- [ ] A decode-robustness test (truncated / bad-tag input ⇒ `Err`, no panic; no
  indexing-panic — restriction-lint clean).

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

### B4. Idiomatic public read-model API + view types + pagination (from A7)

- [ ] Add the `Child` enum + `FolderInfo` / `ItemInfo` view types (typed keys,
  resolved `AssetType` / `InventoryType` / `SaleType` enums) in
  `sl-proto/src/types/inventory.rs`; add the opaque `InventoryCursor`.
- [ ] Add borrowed accessors on `Session`: typed `inventory_folder` /
      `inventory_item` by key,
      `inventory_children(folder) -> impl Iterator<Item = Child<'_>>`,
      `folder_fetch_state(folder)`, `library_root`, and a paged
      `inventory_folder_page(folder, before, limit)` returning the view-type
      window + next cursor (the `history_page` precedent).
- [ ] Deprecate (`#[deprecated]`) the raw `inventory_folders()` /
  `inventory_items()` accessors; keep them compiling for one cycle.
- [ ] Tests: tree-walk over a seeded tree; pagination cursor across a large
  folder; view types carry typed keys + resolved enums.

### B5. Pure cache (de)serialise + merge/version-validity (from A3·A5)

- [ ] Add `sl-proto/src/inventory_cache.rs` (pure, no I/O / gzip / clock): an
      `inventory_to_cache_bytes(&Inventory, owner)` builder returning `Vec<u8>`
      (version header + binary LLSD via B2 + the existing `_to_llsd`
      converters), and an `inventory_from_cache_bytes(&[u8])` parser returning a
      `Result<CachedInventory, _>` (version-check, parse).
- [ ] Add the merge function:
      `(loaded_cache, skeleton) -> (merged_model, folders_needing_fetch)` per A5
      (version-match keeps + `Loaded`, mismatch/absent ⇒ `Unknown`,
      skeleton-absent cached folder dropped).
- [ ] Tests: bytes round-trip to an equal model; version-header mismatch ⇒
      rejected; merge keeps a matching folder (absent from the fetch set) and
      drops a stale one (present in the fetch set); a server-deleted folder is
      dropped.

### B6. Background-fetch orchestration — agent tree (from A6)

- [ ] Add the sans-IO scheduler on `Session`: `next_inventory_fetch_batch() ->
  Vec<InventoryFolderKey>` (BFS over `Unknown`, bounded in-flight), flipping
  returned folders to `Fetching`; descendents replies flip to `Loaded`; an
  on-demand query for an `Unknown` folder enqueues it.
- [ ] A completion query (`inventory_fully_loaded()`), and a re-arm on cache
      merge (the `folders_needing_fetch` set from B5 seeds the queue).
- [ ] Tests: a merged tree with N `Unknown` folders drains over bounded batches
      to fully `Loaded`; an on-demand `Unknown` query schedules exactly that
      folder.

### B7. Library inventory — hold + fetch + separate cache (from A9)

- [ ] Capture the library owner id + library root from the login response into
      the model under `owner = Library` (login carries `library_owner` as
      `Option<Uuid>`, `avatar_profile.rs:353` — wrap as `OwnerKey::Agent`); fold
      the already-emitted `Event::LibraryInventory` (`methods.rs:1213`) skeleton
      into the model under `owner = Library`; add the `library_*` accessors.
- [ ] Fetch the library tree over `FetchLibDescendents2` / `LibraryAPIv3`
  (read-only); reuse B6's scheduler keyed by owner. Persist/load it from the
  separate `<agent-uuid>.lib.inv.llsd.gz` (B5 bytes, owner = Library).
- [ ] Tests: library tree held under its own root, distinct from the agent root;
  the library cache round-trips to its own file; no mutation command targets it.

### B8. `Command`/`Event` pull-bridge (from A8)

- [ ] Add `Command::QueryInventoryFolder { folder, before, limit }` + a roots/
      summary query in `sl-proto/src/command.rs`; add
      `Event::InventoryFolderPage` (carrying
      `{ folder, folders: Arc<[FolderInfo]>, items: Arc<[ItemInfo]>, prev }`)
      and the roots reply in `sl-proto/src/types/event.rs`; handle the queries
      in `methods.rs`.
- [ ] Wire the new commands across the tokio / bevy / REPL dispatch sites at
  parity; bevy continues to read via direct `&Session` borrow.
- [ ] Tests: a folder query replies with the paged view-type window + cursor; an
  `Unknown`-folder query schedules its fetch (ties to B6).

### B9. `ClientDirectories` struct + chat-log retrofit (from A4)

- [ ] Add `ClientDirectories { agent_cache_dir, agent_chat_log_dir,
  shared_cache_dir: Option<PathBuf> }` in `sl-proto` (next to `ChatLogConfig`);
  thread it through the runtime constructors (tokio / bevy / REPL) at parity.
- [ ] Retrofit `ChatLog::new` (`sl-client-tokio/src/chat_log.rs` ~line 157) to
      take `agent_chat_log_dir` **verbatim**, dropping the
      `clean_file_name(own_name)` subdir join; update `ChatLogConfig` if its
      `log_dir` is now redundant.
- [ ] Update the chat-log tests (~lines 619–779) that assume the derived
  `Me Resident` subdir to the verbatim layout.

### B10. Runtime cache shells (tokio + bevy) (from A4·A5·A10)

- [ ] Add an `inventory_cache.rs` runtime shell to each of `sl-client-tokio` and
      `sl-client-bevy`: locate `<agent-uuid>.inv.llsd.gz` / `.lib.inv.llsd.gz`
      **directly** under `agent_cache_dir` (`None` ⇒ caching disabled); gzip via
      new `flate2` dep;
      async (`tokio::fs`) vs blocking I/O. Atomic write (temp + rename).
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
