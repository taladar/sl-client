---
id: inventory-a10
title: Design persistence-vs-region & lifecycle
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A10. Design persistence-vs-region & lifecycle.** Confirm the model is
**not** cleared at the four region-boundary sites (it is grid-level — the chat
persistence-guard precedent). Define save points: on logout and on an
idle/dirty interval (the model tracks a dirty flag set by any fold/mutation),
collecting only `Loaded` folders + their items (skip `Unknown`/`Fetching`),
matching Firestorm `LLCanCache`. Define load timing (at/just-before the
skeleton). The sans-IO `Session` exposes "give me the cacheable snapshot" /
"load this snapshot"; the runtime shell does the file/gzip I/O around it.

## Persistence & lifecycle reference (from A10)

The model is grid-level: add **no** inventory clear at the four region-boundary
sites (`begin_handover`, `promote_child_to_root`, `TeleportLocal`, child
`DisableSimulator`) — the chat persistence-guard precedent (CHAT_ROADMAP B10).
Save points: on logout and on a dirty/idle tick. `Inventory.dirty` is set by
every fold / mutation and cleared on save. The cacheable snapshot collects only
`Loaded` folders + their items (skip `Unknown` / `Fetching`), matching Firestorm
`LLCanCache`. sans-IO surface: a `cacheable_cache_bytes(owner)` builder (A4
bytes) and a `load_cached(owner, Inventory)` intake; the runtime shell owns the
file / gzip I/O and the save timing.

**Persistence guard verified against the code (anchors for B3).** The four
region-boundary handlers touch **only** region-local caches and never reference
inventory: `begin_handover` (`methods.rs:905`) clears `children` /
`child_seeds` / `objects` / `terrain` / `regions` / `time_dilation` and resets
`sit`/in-world
grants, then carries an **explicit** comment (`:949-955`) listing
`chat_sessions` / `friends` / `online` as *deliberately* NOT reset because they
are grid-level (routed by the grid's IM/group/presence services, the inverse of
region-local `sit`) — this is the precedent B3 extends to **name the inventory
model** in the same comment; `promote_child_to_root` (`:1048`), the child
`DisableSimulator` arm (`:1361`, drops only that child's circuit + its
`forget_sim_objects`), and the `TeleportLocal` arm (`:2512`, resets only
`sit`/grants) likewise never touch inventory. Grep-confirmed: the three loose
fields (`inventory_folders` / `inventory_items` / `inventory_root`) are
**never** `.clear()`ed, `= None`'d, or `.take()`n in `sl-proto` — so the guard
holds trivially today; B3 must keep it that way once the loose maps become the
held `Inventory`, and B11 asserts it (mirror the chat
persistence-across-teleport test).

**Save-timing surface verified against the code (anchors for B10).** Neither the
`dirty` flag nor the `cacheable_cache_bytes` / `load_cached` surface exists yet
(grep-empty in `sl-proto`; the only `dirty` in the crate is the unrelated
navmesh state `types/pathfinding.rs:20`). The runtime save-on-logout hook is the
existing terminal-event test: tokio computes
`terminal = matches!(event, Event::Disconnected(_) | Event::LoggedOut)`
(`sl-client-tokio/src/lib.rs:332`) and at `:365-368` aborts the caps task and
returns — the natural point to flush the cache before exit; bevy's terminal path
is `emit_disconnect(..)` → `SlInner::Done` (`lib.rs:442-446`). The inventory
cache shell threads into each runtime exactly where the chat-log writer already
does — constructed at tokio `lib.rs:314` / bevy `lib.rs:404` (both fed
`ClientDirectories` per B9) — so B10 stays at parity with chat-log. The logout
*request* itself is sans-IO `initiate_logout` (`methods.rs:10384`); the
`LogoutReply` arm (`:3867`) emits `Event::LoggedOut`, which is the terminal
event the shell saves on.

**Firestorm grounding (anchors for A10/B10), with two pinned divergences.**
`LLAppViewer::cleanup` caches the agent tree then the library tree
(`llappviewer.cpp:6624-6638`) **only at shutdown** — there is **no** periodic
inventory save in Firestorm. `LLInventoryModel::cache(root, owner)`
(`llinventorymodel.cpp:2556`) collects descendents under the root through the
`LLCanCache` predicate (`:135-155`: a category is cacheable iff
`getVersion() != VERSION_UNKNOWN` **and** `descendents_server ==
descendents_actual`, an item iff its parent is cacheable), writes a temp
file via `saveToFile` (`:2580`) — which **re-filters** categories on
`getVersion() != VERSION_UNKNOWN` (`:3814`) — then `gzip_file`s it. Two
divergences B10 must own: (1) **the dirty/idle tick is beyond Firestorm** — it
saves only at shutdown; A10 adds an *optional* dirty/idle save purely for
crash-safety, made safe by A4's atomic temp+`fsync`+`rename`, and the `dirty`
flag exists to *skip* a no-op rewrite (not a Firestorm requirement). (2) **the
library second-instance guard** — Firestorm writes the library cache only when
`!mSecondInstance` ("agent is unique, library isn't", `:6631`); the
single-process sl-client has no second-instance contention and A4's atomic write
already prevents torn writes, so B10 writes both caches unconditionally. Our
`Loaded`-only snapshot subsumes both Firestorm filters: we set
`Loaded { version }` only on a complete descendents reply, so
`getVersion() != VERSION_UNKNOWN` and the descendent-count match collapse into
the one `FolderState::Loaded` gate.
