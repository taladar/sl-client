---
id: inventory-a6
title: Design background-fetch orchestration
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A6. Design background-fetch orchestration.** A breadth-first walk over
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

## Background-fetch reference (from A6)

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
