---
id: test-fake-grid-imitates-sl-new-file-upload-announcement
title: The one upload announcement Second Life would only tell us for money
topic: test
status: ready
origin: the measured residue of test-fake-grid-imitates-upload-announcements (2026-09-08)
points: 3
refs: [test-fake-grid-imitates-upload-announcements, test-fake-grid-imitates-economy]
---

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
