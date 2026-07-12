---
id: inventory-b6
title: Background-fetch orchestration
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B6. Background-fetch orchestration — agent tree (from A6)

- [x] Add the sans-IO scheduler on `Session`: `next_inventory_fetch_batch(&mut
  self, max_in_flight) -> Vec<InventoryFolderKey>` (BFS over `Unknown`, bounded
  in-flight), flipping returned folders to `Fetching`; the existing descendents
  folds (CAPS `methods.rs:403` / UDP `:2406`, both via `cache_inventory`) flip
  the target to `Loaded { version }` (version from `reply.agent_data.version`)
  and seed new children `Unknown`; an on-demand query for an `Unknown` folder
  reuses the existing `request_folder_contents` (`methods.rs:7833`).
- [x] Add the **opt-out gate** (user requirement): a sans-IO flag
  `background_inventory_fetch: bool` on `Session` with
  `set_background_inventory_fetch(&mut self, bool)`, **default `false`** — while
  off, `next_inventory_fetch_batch` returns empty and nothing auto-enqueues (so
  `sl-survey` pays nothing). The flag gates **only** the automatic BFS; the
  explicit pulls (`request_folder_contents`, `Command::FetchInventoryFolders`)
  still work when off. Expose it through the runtime inventory config at parity
  (tokio / bevy / REPL); the runtime crawl tick (after merge, after each reply)
  calls the scheduler only when enabled. Leave `sl-survey` on the default-off
  path (no enable call needed).
- [x] A completion query (`inventory_fully_loaded(owner)` — no `Unknown` /
      `Fetching` under that owner), and a re-arm on cache merge (the
      `folders_needing_fetch` set from B5 seeds the queue).
- [x] Tests: a merged tree with N `Unknown` folders drains over bounded batches
      to fully `Loaded`; an on-demand `Unknown` query schedules exactly that
      folder; with the gate **off**, `next_inventory_fetch_batch` returns empty
      even with `Unknown` folders present while `request_folder_contents` still
      schedules its one folder.

**Landed (notes for later tasks).** The scheduler lives in the held model
(`Inventory::next_fetch_batch` / `mark_folder_fetching` / `fully_loaded` in
`session/inventory.rs`), surfaced by gated `Session` wrappers
(`next_inventory_fetch_batch` self-returns empty when the flag is off,
`set_background_inventory_fetch` / `background_inventory_fetch` /
`inventory_fully_loaded`). Refinements from the drafted shape, all internal: (1)
**no explicit fetch queue / re-arm needed** — the batch is *derived* from
`Unknown` states each call, so `merge_skeleton`'s `Unknown` set (B5) and the new
children a reply seeds are picked up automatically; "re-arm on cache merge" is
therefore free. (2) `request_folder_contents` now also flips its folder
`Fetching` so the scheduler will not re-pick an on-demand fetch and
`inventory_fully_loaded` reflects it. (3) The in-flight bound is the new crate
const `INVENTORY_FETCH_MAX_IN_FLIGHT = 12` (Firestorm legacy
`max_concurrent_fetches`), passed by the runtimes. (4) Runtime crawl tick added
at parity — tokio at the top of the `run` loop, bevy after the CAPS-event drain
in `advance_running` — each gated on the fetch cap + agent id being known (so a
folder is never flipped `Fetching` for a request that cannot be issued) and
spawning the existing `fetch_inventory` / `run_inventory_fetch` POST. The full
REPLs enable it (`set_background_inventory_fetch(true)` / plugin field `true`);
`sl-survey` and the two bevy examples leave it `false`. **Library-tree fetch
(`FetchLibDescendents2`, owner-keyed) reuses this same scheduler but is wired in
B7;** today the crawl only POSTs the agent tree (owner = agent id). A stuck POST
(network failure, no reply) leaves its folder `Fetching` indefinitely — no
fetch-timeout/retry yet (Firestorm has one); acceptable for now, revisit if a
live crawl stalls.
