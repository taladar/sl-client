---
id: protocol-account-benefits-package
title: What an upload actually costs is in the benefits package nothing decodes
topic: protocol
status: ready
origin: asked while reviewing test-fake-grid-imitates-economy (2026-09-08)
points: 3
refs:
  [
    test-fake-grid-imitates-economy,
    test-fake-grid-imitates-sl-new-file-upload-announcement,
  ]
---

Context: [context/protocol.md](../context/protocol.md).

[[test-fake-grid-imitates-economy]] measured `EconomyData` and found Second Life
answering `price_upload = 10`. That number is real, and on Second Life it is
also **not what anybody is charged**.

The modern reference viewer prices uploads from the login response's
`account_level_benefits`, not from `EconomyData`. `LLAgentBenefits` reads
`texture_upload_cost`, `sound_upload_cost`, `animation_upload_cost` and
`create_group_cost` out of it, and `getTextureUploadCost()` returns the benefits
value outright. Firestorm's `<FS:Ansariel> OpenSim legacy economy` patches are
what fall back to `LLGlobalEconomy::getPriceUpload()` — the `EconomyData` field
— and they do so **only when the grid is not Second Life**. So the two grids use
two different sources for one number, and this workspace models only the one
OpenSim uses.

Three things live there that `EconomyData` cannot express at all:

- **Tiered texture pricing.** `large_texture_upload_cost` is an array, sorted
  ascending, and `get2KTextureUploadCost` returns its first entry. It applies
  when a texture's area is at least `MIN_2K_TEXTURE_AREA = 1024 * 1024 + 1` —
  i.e. anything above 1024×1024 costs more than the flat rate. `EconomyData`
  has one `price_upload` field and no way to say this.
- **Per-plan pricing and limits.** `account_type` names the account's package
  ("Base", "Premium", "Premium_Plus"), `account_level_benefits` is that
  package's numbers, and `premium_packages` is a map of *every* package so a
  viewer can render "Premium would give you N" against what you have.
  `LLAgentBenefitsMgr::get("Premium").getGroupMembershipLimit()` versus
  `current()` is exactly how the reference viewer draws its upgrade prompts, and
  `isCurrent("Base")` picks between the "Upgrade" and "Premium" menu labels.
- **Limits that are not prices**: `attachment_limit`, `group_membership_limit`,
  `animated_object_limit`, `picks_limit`. A viewer that enforces or displays any
  of these has nowhere else to read them.

`sl-wire` already carries all three fields — `LoginSuccess::account_type`,
`account_level_benefits`, `premium_packages` — but the latter two are opaque
`Llsd` blobs, and **nothing in the workspace reads any of them**: no decode, no
typed accessor, no viewer consumer, no fake-grid answer.

Wanted:

- A typed `AccountBenefits` in the pure crate decoded from
  `account_level_benefits`, with the package map beside it, and the same
  area-threshold dispatch the reference viewer uses so a texture's cost is a
  function of its dimensions rather than a constant.
- The fake grid answering it, flavour-decided: Second Life sends a benefits
  package, a stock OpenSim grid sends none at all — which is the honest reason
  the legacy `EconomyData` path exists, and a better divergence than most
  because the *absence* is what the OpenSim side has to model.
- A measurement first, as ever: nothing in this workspace has ever recorded what
  aditi's `account_level_benefits` actually contains, so every number above is
  read off the viewer source rather than off a grid. The test avatar is a Base
  account, so `premium_packages` is the only way to see the Premium figures
  without buying one.

Acceptance: an aditi login response's benefits package is recorded; the cost of
uploading a 2048×2048 texture is derivable from decoded types rather than from
a flat `price_upload`; and a fake grid can be either a grid that has benefits or
one that does not.
