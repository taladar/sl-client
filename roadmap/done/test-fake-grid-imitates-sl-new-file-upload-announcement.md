---
id: test-fake-grid-imitates-sl-new-file-upload-announcement
title: The one upload announcement Second Life would only tell us for money
topic: test
status: done
origin: the measured residue of test-fake-grid-imitates-upload-announcements (2026-09-08)
points: 3
refs: [test-fake-grid-imitates-upload-announcements, test-fake-grid-imitates-economy]
---

Done 2026-09-08. See "What landed" below — the expected answer was
wrong.

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-imitates-upload-announcements]] measured three of the four
cells of the upload-announcement table and derived `UploadAnnouncement` from
them. This is the fourth: **what Second Life sends after a
`NewFileAgentInventory` completion**, as opposed to after an in-place save.

The other three were free. This one is not, and that is the whole reason it is
a separate item: on Second Life that capability serves only the chargeable
file-upload classes — it answers a notecard with `Invalid asset type`, which is
why `asset-upload` records `partial` there — so reaching the completion at all
means uploading a texture (or a sound, or an animation) and paying the grid's
upload fee out of the test avatar's aditi balance. That in turn means sending
the *right* `expected_upload_cost`, which the grid checks, and the price list
was [[test-fake-grid-imitates-economy]]'s measurement to take.

**That measurement is now taken** (2026-09-08), but it is the wrong number to
spend. Second Life answers `price_upload = 10` on aditi and
`ImitatedGrid::prices` carries it — and on Second Life the reference viewer does
not price uploads from that field at all. `LLAgentBenefits` reads
`texture_upload_cost` out of the login response's
`account_level_benefits`, and falls back to `EconomyData`'s
`price_upload` only off Second Life. Worse for this item, the benefits package
prices large textures separately: `large_texture_upload_cost` applies above
`MIN_2K_TEXTURE_AREA` (1024×1024), which a single `price_upload` cannot express.

So the ordering is satisfied in the sense that the legacy field is measured, but
the `expected_upload_cost` this run has to send is the *benefits* figure, and
nothing in this workspace decodes that yet —
[[protocol-account-benefits-package]]. Two ways out, and the first is cheaper:

- upload a **small** texture (≤ 1024×1024, well under the tier) and read the
  cost from the login response's `account_level_benefits.texture_upload_cost`
  directly out of the `Llsd` blob `sl-wire` already keeps, without waiting for
  the typed decode; or
- do [[protocol-account-benefits-package]] first and take the cost from typed
  accessors.

Either way, do not take it from `Event::EconomyData` or from
`second_life_prices()`: the grid checks the value it is sent, and the field this
task's sibling measured is the one Second Life stopped charging from.

**The expected answer is "the same legacy push", and that is why this is a
confirmation rather than an open question.** The message Second Life was
measured sending after an in-place save is the general-purpose legacy
`UpdateCreateInventoryItem` — the one that has always announced a created *or*
rewritten item — and OpenSim's silence turns out to be OpenSim's own omission
(that call has been commented out in its source since 2007-08, and the
`NewFileAgentInventory` completion uses the client-less `AddInventoryItem`
overload) rather than a newer protocol Second Life has yet to catch up with.
A grid that kept the push for a rewritten item and dropped it for a created one
would be the odd one, so a measurement that finds anything else is the
interesting outcome and should be treated as a result, not as a broken case.

- Take the upload price from `Event::EconomyData`'s `price_upload` rather than
  hard-coding one, and skip (`partial`, not fail) when the avatar's balance
  will not cover it — a case that silently spends the test avatar's last L$ is
  worse than one that says why it did not run.
- Upload a `sl-test-assets` texture through `NewFileAgentInventory` on aditi,
  watch it with `support::observe_upload` (the instrument already exists), and
  record `upload_announcement` next to the OpenSim `none` that case already
  holds.
- Then either confirm that Second Life announces a new item the way it
  announces a rewritten one — which is what `ImitatedGrid::upload_announcement`
  currently extrapolates, and what the `sl_fake_grid::inventory` docs say is
  extrapolated — or split `UploadAnnouncement` so the two paths can differ, and
  correct the docs in `inventory.rs`, `imitates.rs`, the README and the book.

Acceptance: `asset-upload` carries an aditi `upload_announcement` that is a
measurement rather than a decline, and the extrapolation note in
`sl_fake_grid::inventory` is either deleted as confirmed or replaced by the
second policy the measurement demands.

## What landed

**The measurement, and it came back the way this item called "the interesting
outcome".** Second Life announces **nothing** after a `NewFileAgentInventory`
completion — the same silence OpenSim keeps there, and the opposite of the
legacy push it was measured sending after an in-place save. Two paid runs
(aditi, 2026-09-08) recorded `upload_announcement = none`, so
`UploadAnnouncement` was split rather than confirmed.

| after a capability upload | Second Life | OpenSim |
| --- | --- | --- |
| in-place save (`Update*AgentInventory`) | the legacy UDP `UpdateCreateInventoryItem` | nothing at all |
| a `NewFileAgentInventory` completion | **nothing at all** | nothing at all |

**Why the extrapolation was wrong, which is the part worth keeping.** It
reasoned from the message: the push is the general-purpose legacy "here is an
item you now have", the grid sends exactly it one path over, so dropping it for
a *created* item would be the odd behaviour. What that misses is **what the
client already holds**. A `NewFileAgentInventory` response body carries the
whole new item, so a push would repeat it; an in-place save's response names
only the new *asset*, and without the push the client's own copy of the item
goes on naming the asset the save replaced. The push survives exactly where it
still carries information — and the two grids' agreement on the creation path
is a coincidence of two different omissions rather than a shared rule, which is
why the knob is a pair of values and not one.

**The split.** `UploadAnnouncements { created, saved }` in
`sl-fake-grid/src/inventory.rs`, resolved by
`ImitatedGrid::upload_announcements` and overridable with
`FakeGridBuilder::upload_announcements` (`UploadAnnouncements::uniform` for a
grid that should treat the paths alike). `uploads.rs` reads `.created` at the
`NewFileInventory` arm and `.saved` at the two `Update*Agent*` arms;
`Default` is hand-written, because the field-wise default of
`UploadAnnouncement` is the legacy push and the Second Life pair is not that.
`client_end_to_end`'s
`no_flavour_announces_an_item_a_capability_upload_created` asserts the new half
from the client's end on both flavours, terminating on a task-inventory listing
requested after the completion so "nothing arrived" stays a bounded claim.

**What the case does now.** `asset-upload` uploads a 64×64 checkerboard texture
on Second Life (the notecard it uploads on OpenSim is refused there with
`Invalid asset type`) and a notecard on OpenSim, each into the system folder for
its class rather than the inventory root. The fee comes from the login
response's benefits package via the new `Session::login_account` —
`texture_upload_cost_for(64, 64)`, i.e. the flat rate, the texture being far
below `MIN_2K_TEXTURE_AREA` where the price would be five times higher — and the
case declines the run (`partial`) rather than spending money it does not have.
Both aditi runs recorded `upload_charged = 10` against `upload_cost = 10`, which
is the first *direct* confirmation that Second Life bills from the benefits
package and not from `EconomyData::price_upload`:
[[protocol-account-benefits-package]] established what the two fields say, and
this establishes which one the grid takes money by. The record also carries
`upload_asset_class` and
`upload_cost_source`, so a reader can see the two grids were measured on
different classes.

**Two accessors it needed.** `Client::login_account` in `sl-client-tokio` and
`Session::login_account` in the conformance harness, both because
`Event::Account` is easy to lose: it arrives as the login response is parsed,
and a case that waits for a region handshake first discards it on the way past —
which a case that must know a price *before* it sends the upload cannot afford.

**One incidental observation, not chased.** Both runs uploaded byte-identical
J2C and both got asset id `4f83ed1c-…` back, with a fresh item id each time and
a fresh L$ 10 charged each time. So Second Life appears to content-address an
uploaded asset while still minting and billing a new item — worth knowing before
anyone treats "the asset id changed" as proof an upload happened.
