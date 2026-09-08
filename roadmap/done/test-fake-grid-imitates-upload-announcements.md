---
id: test-fake-grid-imitates-upload-announcements
title: An uploaded item is announced the legacy way whichever grid this is
topic: test
status: done
origin: the honest residue of test-fake-grid-imitates-inventory-api (2026-09-07)
points: 2
refs:
  [
    test-fake-grid-imitates-inventory-api,
    test-fake-grid-imitates-sl-new-file-upload-announcement,
    viewer-uploaded-item-never-enters-the-model,
  ]
---

Done 2026-09-08. See "What landed" below.

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-imitates-inventory-api]] gave the fake grid
`InventoryAnnouncement`, and a **take** follows it: the legacy UDP
`UpdateCreateInventoryItem` on an OpenSim-flavoured grid, a
`BulkUpdateInventory` over the event queue on a Second-Life-flavoured one.

The upload paths in `sl-fake-grid/src/uploads.rs` do not. A
`NewFileAgentInventory` completion, an in-place asset save
(`UpdateAgentItem`) and the legacy `UpdateInventoryItem` transaction all still
announce with the legacy message on both flavours.

That was left deliberately rather than derived, because it is **unmeasured**.
Each of those is the reply to a capability the client called, and the HTTP
response already carries the new item id and asset id — the reference viewer
builds the item from the response body rather than waiting for a push — so it
is genuinely unclear whether a Second Life simulator announces it again over
the event queue, announces it over UDP anyway, or says nothing at all.
Deriving a guess from the flavour would be inventing behaviour, which is the
one thing `ImitatedGrid` is not for.

So this is a measurement first and a derivation second, the same shape as
[[test-fake-grid-imitates-economy]]:

- On aditi, upload a notecard through `NewFileAgentInventory` and save over an
  existing item's asset, and record what arrives besides the HTTP response —
  `asset-upload` and `script-upload` already do the uploads, so what is missing
  is watching the event stream around them rather than new machinery.
- Then either derive the announcement from `ImitatedGrid` the way the take
  does, or write down that both grids answer these the same way and delete
  this item.

Acceptance: what Second Life sends after an upload completion is recorded, and
`uploads.rs` either follows the flavour or documents why it does not need to.

## What landed

The measurement, and it came back **the opposite way round from the take**,
which is why this ended in a second policy type rather than one more caller of
`InventoryAnnouncement`.

| after a capability upload | Second Life | OpenSim |
| --- | --- | --- |
| in-place save (`Update*AgentInventory`) | the legacy UDP `UpdateCreateInventoryItem` | nothing at all |
| a `NewFileAgentInventory` completion | not measurable free (see below) | nothing at all |

> **Corrected 2026-09-08 by
> [[test-fake-grid-imitates-sl-new-file-upload-announcement]].** The paid run
> was made and the second row's Second Life cell is **nothing at all** — the
> extrapolation below reasoned from the message and should have reasoned from
> what the client already holds. `UploadAnnouncement` was split into an
> `UploadAnnouncements` pair as a result. Everything else here stands.

So on a take Second Life pushes a `BulkUpdateInventory` and OpenSim sends the
legacy message; after an upload it is Second Life that sends the legacy message
and OpenSim that says nothing. The fake grid's unconditional legacy
announcement was therefore not "the wrong one of two" — it matched **neither**
live grid on the OpenSim side.

**The instrument.** `support::observe_upload` watches the event stream around
an upload instead of awaiting its completion, because a plain `wait_for` cannot
answer this question: it discards every event its predicate rejects, and an
announcement is a UDP push while a completion is an HTTP response, so an
announcement arriving first would be eaten on the way past and the grid
recorded as silent. `drain_announcements` is the same watch without the
completion, used between the create and the save so an announcement seen during
the save is the save's (`create_trailing_announcements_count` records what it
caught: zero on both grids). `notecard-create-update` records
`save_announcement` on both grids and `asset-upload` records
`upload_announcement` on OpenSim.

**The derivation.** `UploadAnnouncement { Legacy, Silent }` in
`sl-fake-grid/src/inventory.rs`, derived from `ImitatedGrid` and overridable
with `FakeGridBuilder::upload_announcement`. Both capability paths in
`uploads.rs` read it. The legacy UDP transaction save deliberately does not:
its `UpdateCreateInventoryItem` is the **reply** to a UDP request, echoing the
transaction and callback ids the client sent, and a client's
`Command::SaveInventoryAsset` has nothing else to complete on — OpenSim sends
it there (`AssetXferUploader`) exactly where it stays quiet after a capability
upload. `client_end_to_end`'s
`only_a_second_life_flavoured_grid_announces_an_uploaded_item` (renamed
`…_an_item_an_upload_rewrote` by the correction above) asserts both
sides from the client's end, the OpenSim half terminating on a task-inventory
listing requested after the save so that "nothing arrived" is a bounded claim
rather than a wait for a timeout.

**Which way the divergence points**, which the raw table does not say and
getting backwards would mislead the next reader: the push is the **older**
behaviour and OpenSim is the grid that omits it. Its
`SendInventoryItemCreateUpdate` at the in-place save has been commented out
since 2007-08, when that capability
path was first written (carried through the 2007-12 rename and the 2010 move
into `InventoryAccessModule` still commented), and its `NewFileAgentInventory`
completion reaches inventory through the *client-less* `AddInventoryItem`
overload sitting beside the one that announces. So the take's divergence is
Second Life having moved on to AIS3, and this one is OpenSim having never sent
what a Linden simulator sends.

**What could not be measured, and what was done instead.** This item planned to
upload a notecard through `NewFileAgentInventory` on aditi. That is not
possible: Second Life accepts only the chargeable file-upload classes there and
answers a notecard with `Invalid asset type` — which `asset-upload` already
recorded as a `partial` before this item existed. Reaching that completion
needs an upload fee, and the right `expected_upload_cost` needs the price list
[[test-fake-grid-imitates-economy]] has yet to measure. So the Second Life side
of the new-file row is extrapolated from the in-place row — same grid, same
capability family, and the message in question is the general-purpose legacy
"here is an item you now have" — and
[[test-fake-grid-imitates-sl-new-file-upload-announcement]] carries the run
that would confirm it, with that expectation written down.

**A viewer gap this exposed**, filed as
[[viewer-uploaded-item-never-enters-the-model]]: `sl-viewer-inventory` learns a
new item only from an announcement, so against a grid that announces nothing
(OpenSim, measured) an uploaded item does not appear in the inventory model at
all until the folder is re-fetched. The reference viewer does not have this
problem because it builds the item from the capability's response body.

**One bug fixed in passing:** `asset-upload`'s `upload_secs` was started before
the watch, so the first run recorded the 15-second settle window as part of the
upload (15.04 s). It records the completion's own round trip now.
