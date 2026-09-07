---
id: test-fake-grid-imitates-inventory-api
title: The fake grid is OpenSim about inventory whichever grid it says it is
topic: test
status: done
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 5
refs:
  [test-fake-grid-object-asset-id-divergence, protocol-ais3-nested-embedded]
---

Done 2026-09-07. See "What landed" below.

Context: [context/testing.md](../context/testing.md).

`ImitatedGrid` picks which live grid the fake one is, and inventory is the
loudest divergence it does **not** yet decide. Both halves are OpenSim's, on a
grid that says it is Second Life:

- **The fetch.** OpenSim still serves the deprecated UDP
  `FetchInventoryDescendents`; Second Life silently drops it and a viewer must
  use AIS3. The fake grid serves it on neither — `LegacyUdpInventory` picks
  between *refusing* it with a `FeatureDisabled` and *ignoring* it, which are
  the two roads a grid that does not serve it has. So a viewer that still
  reached for the UDP path would be failed here and would work on OpenSim,
  which is the wrong way round for a grid claiming to be OpenSim.
- **The announcement.** A take is answered with the legacy
  `UpdateCreateInventoryItem` on both flavours. Second Life moved inventory to
  AIS3 and delivers the new item as a `BulkUpdateInventory` over the event
  queue instead. `object-asset-format`'s take leg already waits for *either*,
  precisely because the two grids differ — the comment there records it.

Neither side is a flag away, which is why the flavour does not claim to decide
them: `SimSession` has **no sender for either message**. It has
`send_inventory_item_created` (the legacy one) and nothing else. So this is two
senders in `sl-proto` first, then the derivation:

- `SimSession::send_inventory_descendents`, answering a UDP
  `FetchInventoryDescendents` out of `SimInventoryTree` — which already has
  `descendents` for the AIS3 side to build on.
- `SimSession::enqueue_bulk_update_inventory`, the event-queue form, which is
  what a Second-Life-flavoured take should hand back.

Then `ImitatedGrid` grows the pair of them — how a folder is fetched, and how a
new item is announced — and `LegacyUdpInventory` stops being a builder knob a
caller has to reason about separately: an OpenSim-flavoured grid serves the UDP
fetch, a Second-Life-flavoured one drops it.

Worth doing because it is the divergence a *viewer* is most likely to trip
over: an inventory implementation that quietly depends on the legacy path
passes every offline case today.

Acceptance: `ImitatedGrid` decides both the fetch and the announcement; a
conformance case fetches a folder over UDP against `Grid::FakeOpensim` and is
refused against `Grid::FakeSl`, and a take is announced as a
`BulkUpdateInventory` on the one and an `UpdateCreateInventoryItem` on the
other.

## What landed

Both halves, plus the typed event the fetch needed to be answerable at all.

**`sl-proto`.** The deprecated UDP fetch stopped being a raw forward: it is
`ServerEvent::RequestInventoryDescendents` now, which took
`FetchInventoryDescendents` off the `RAW_FORWARDED` ledger in
`tests/sim_session_symmetry.rs`. Two senders answer it:

- `SimSession::send_inventory_descendents` reads the folder out of whichever
  serving tree holds it (the agent's, else the Library's — the same two
  `SimCaps` serves `FetchInventoryDescendents2` from) and packs the reply the
  way `LLClientView.SendInventoryFolderDetails` does: at most six folders or
  five items per message, folders and items **never in the same message** ("to
  preserve SL compatibility", says the comment there), and a nil-id placeholder
  block padding whichever half is empty — so an empty folder is one message of
  two placeholders rather than no message. It returns `false`, having sent
  nothing, for a folder neither tree has.
- `SimSession::enqueue_bulk_update_inventory` wraps the
  `bulk_update_inventory_to_llsd` encoder that already existed for the client's
  own round-trip test and pushes it over the event queue.

The placeholder blocks are worth the paragraph: the client has always filtered
them on their nil ids (`InventoryDescendents` arm in `session/methods.rs`), and
nothing in the workspace ever produced one, so the filter was unreachable until
now.

**`sl-fake-grid`.** A new `inventory` module holds both policy enums, because
they are one divergence seen from either end: `LegacyUdpInventory` (which gained
a third road, `Served`) and `InventoryAnnouncement`. Both are derived from
`ImitatedGrid` and both are overridable per grid, like every other knob it
decides. The Second Life side of the fetch is the **refusal**, not the silence
aditi was measured giving — of the two roads a grid without the path has, only
the refusal leaves a test something to assert — and that deliberate deviation is
written down in `imitates.rs`, the README and the book rather than left as an
accident.

**What it caught, immediately and exactly as predicted.** Flipping the
announcement default to Second Life broke `object-rez-derez` and
`task-inventory`: both waited only for `Event::InventoryItemCreated` and
reported a take that had worked as unacknowledged. That is precisely the
failure a viewer depending on the legacy path would have had against the grid
this workspace targets, and it was invisible while the fake grid answered both
flavours the same way. The fix is one shared helper —
`support::created_item_announcement`, which accepts either shape and says which
arrived — used by every case that takes something, `attach-detach` included
(that one is live-grid only and had the same wait, so it would have timed out
on aditi for the same reason).

**And a second-order one nothing had predicted: the announcement's *order*
changes with it.** A take sends the filed item and the world's `KillObject`s in
one breath, but the kills go out over UDP immediately while a
Second-Life-flavoured announcement rides the event queue and lands on the next
long-poll — so on that flavour the item arrives **after** the kills.
`a_taken_linkset_rezzes_back_whole` waited for the item and *then* for the two
kills, so it silently ate both on the way past and then waited forever for
them; `wait_on`'s timeout is per event, and a grid still sending pings never
trips it ([[sl-client-fake-grid-follow-ups]] records that trap). It drains for
all three in one pass now, and the ordering is written down in
`sl-fake-grid/src/inventory.rs` rather than left for the next consumer to
rediscover the same way.

**Acceptance, case by case.** `server-error` now declares both fake flavours:
`FakeSl` refuses the probe with a `FeatureDisabled` (unchanged), `FakeOpensim`
serves it and the case asserts the reply actually carries the folder's children,
because a reply naming the folder with nothing in it would mean the switch
flipped without the sender reaching the inventory. `object-asset-format` records
`take_announcement` and asserts it against `grid.behaves_like()`, so a fake grid
announcing with the wrong message fails rather than being papered over by the
accept-either helper.
