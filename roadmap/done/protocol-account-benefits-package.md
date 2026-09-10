---
id: protocol-account-benefits-package
title: What an account is entitled to arrives at login, and nothing decodes it
topic: protocol
status: done
origin: asked while reviewing test-fake-grid-imitates-economy (2026-09-08)
points: 5
refs:
  [
    test-fake-grid-imitates-economy,
    test-fake-grid-imitates-sl-new-file-upload-announcement,
    viewer-search-maturity-filter,
    viewer-region-entry-maturity-gate,
  ]
---

Done 2026-09-08. See "What landed" below.

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

**The viewer hard-requires two packages.** `init_benefits` fails outright if
`premium_packages` lacks `Base` or `Premium`, and a failed init pops
`FailedToGetBenefits` at `STATE_CLEANUP`. So a Second-Life-flavoured fake grid
cannot send only the current package: it has to send the map, or a reference
viewer pointed at it nags on every login. Firestorm avoids that against OpenSim
by gating the whole `init_benefits` call behind `isInSecondLife()` and setting
`mBenefitsSuccessfullyInit = true` unconditionally otherwise — which is the
clearest statement available that an OpenSim login response carries none of
this.

## The maturity trio is the same shape

Folded in here rather than filed separately, because it is the same mechanism —
login-response fields whose *presence* or per-account-ness is the divergence —
and splitting it would mean touching `LoginSuccess` and `ImitatedGrid` twice for
one job.

- **`agent_region_access` — Second Life sends it, OpenSim never does.** Zero
  hits across the whole OpenSim tree. It is the account's *preference* value
  ("always `<= agent_access_max`"), and the reference viewer seeds
  `PreferredMaturity` from it. Firestorm patches around its absence explicitly,
  naming the ticket: *"FIRE-8854: Set the preferred maturity here to the maximum
  in case the sim doesn't send it at login, like OpenSim doesn't."* Nothing in
  this workspace reads the field at all, so the initial preference is not seeded
  from the grid on either flavour — we silently inherit the fallback without
  having implemented the thing it falls back from.
- **`agent_access` / `agent_access_max` — both grids send them, but they mean
  different things.** OpenSim hard-codes `agentAccess = "M"` and
  `agentAccessMax = "A"` for every account; there is no per-account maturity in
  OpenSim at all. On Second Life they are real per-account values. The fake grid
  currently hard-codes exactly OpenSim's pair (`runtime.rs`), on **both**
  flavours — so a Second-Life-flavoured grid is lying, and every fake account is
  maximally entitled.
- That last point has teeth: `sl-viewer-preferences` reads `agent_access_max` as
  the account's ceiling and implements the reference `canSetMaturity` rule
  against it. With the ceiling pinned to `"A"`, **that rule has never been
  exercised against a restricted account**, and neither has any refusal path
  that depends on one ([[viewer-region-entry-maturity-gate]]).

Wanted:

- A typed `AccountBenefits` in the pure crate decoded from
  `account_level_benefits`, with the package map beside it, and the same
  area-threshold dispatch the reference viewer uses so a texture's cost is a
  function of its dimensions rather than a constant.
- The maturity trio decoded to the existing `Maturity` type rather than left as
  `Option<String>`, with `agent_region_access` seeding the initial
  `PreferredMaturity` and the FIRE-8854 fallback — preference := ceiling when
  the grid sent no preference — implemented deliberately rather than by
  omission.
- The fake grid answering all of it, flavour-decided, each flavour behaving as
  the grid it imitates. Second Life: a benefits package (with `Base` and
  `Premium` present in `premium_packages`, or the reference viewer complains),
  `agent_region_access`, and per-account `agent_access`/`agent_access_max` from
  `AccountConfig` so a restricted account is expressible. Stock OpenSim: no
  benefits package, no `agent_region_access`, and the hard-coded `M`/`A` pair
  for everyone. The *absence* is what the OpenSim side has to model, which is
  the same divergence-of-presence shape as the `OpenSimExtras` block and the
  currency symbol.
- A measurement first, as ever: nothing in this workspace has ever recorded what
  aditi's `account_level_benefits` actually contains, so every number above is
  read off the viewer source rather than off a grid. The test avatar is a Base
  account, so `premium_packages` is the only way to see the Premium figures
  without buying one — and it is also the only way to check the claim that the
  2K tier is L$ 50.

Acceptance: an aditi login response's benefits package and maturity trio are
recorded; the cost of uploading a 2048×2048 texture is derivable from decoded
types rather than from a flat `price_upload`; a fake account can be given a
maturity ceiling below Adult and the preference machinery refuses to exceed it;
and each fake flavour sends exactly the login fields its grid sends.

## What landed

**The measurement, and the L$ 50 figure is confirmed.** `login-handshake` now
records the whole account picture, and aditi answers (2026-09-08):

| package | texture | 2K texture | sound/anim | group | groups | animesh |
| --- | --- | --- | --- | --- | --- | --- |
| `Base` | 10 | **50** | 10 | 100 | 50 | 1 |
| `Plus` | 10 | 50 | 10 | 100 | 55 | 1 |
| `Premium` | 10 | **40** | 10 | 100 | 80 | 2 |
| `Premium_Plus` | **0** | **0** | **0** | **10** | 150 | 3 |
| `Premium_Plus_No_Stipend` | 0 | 0 | 0 | 10 | 150 | 3 |

So a subscription buys less than the marketing implies and in fewer places:
uploads are free only at Premium Plus, the 2K tier is the one price that steps
gradually (50 → 40 → 0), group creation drops only at the top, and the group
limit is the only figure that moves at every tier. `picks_limit` (20) and
`attachment_limit` (38) are identical across all five — a viewer gating either
on the subscription would be gating on nothing. Aditi sends **five** packages,
not the two the reference viewer demands.

**The decode.** `AccountBenefits` in `sl-proto`, with the reference parse's own
strictness (all eight integer fields required; a missing one rejects the whole
package rather than defaulting a price to zero) and the `MIN_2K_TEXTURE_AREA`
dispatch, so a texture's cost is a function of its dimensions.
`LoginAccount` gained `preferred_maturity`, `account_type`, `benefits` and
`packages`; `Maturity` gained `to_login_access` (the missing encoder half) and
`permitted_by`, where `Unknown` is permitted by nothing because `true` is the
unsafe guess.

**The fake grid.** `ImitatedGrid::describes_account_entitlements` resolves into
a `GridCore` knob like every other flavour decision; the measured five-package
table is served back out; `AccountConfig` gained `maturity_ceiling`,
`preferred_maturity` and `package`. Verified by conformance run rather than
asserted: **fake-sl now matches aditi field for field**, and fake-opensim sends
none of it.

**The ceiling knob is the one with teeth.** Every fake account was previously
entitled to everything (`agent_access_max` hard-coded `"A"`), so the client's
`canSetMaturity` rule — which reads exactly that field — had never once been
exercised against an account that could fail it. That is what
[[viewer-region-entry-maturity-gate]] was blocked on.

**Two things I got wrong and the measurement corrected.**

- `agent_access` is not the preference, which is the reading its name invites
  and what the first implementation sent. Aditi answers `M` on three runs whose
  ceiling *and* preference are both `A`.
- Hoisting the clamp out of the flavour branch made the OpenSim flavour honour
  a ceiling it must ignore. The new
  `an_opensim_flavoured_grid_describes_no_entitlements` caught it, which is a
  test earning its place on its first run.

**What `agent_access` actually is, still open**, and parked rather than chased:
[[protocol-agent-access-meaning]] in `deferred/`, because settling it needs an
age-verified avatar or a differently-rated start region — manual arrangement,
and nothing is blocked on the answer. Two readings survive: a *clearance* (what
the account is cleared for as against what its type permits), or the **start
region's own rating**. The case records `start_region_maturity` to separate them
and the answer came back ambiguous: that avatar's start region is itself Mature.
It does rule out a vestigial constant — the value coincides with something
rather than sitting where it was left.

**A doc bug fixed in passing:** `sl-wire`'s `agent_region_access` was documented
as "the maturity rating of the region the avatar starts in". It is the account's
*preference*; the reference viewer seeds `PreferredMaturity` from it.
