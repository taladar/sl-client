---
id: test-fake-grid-imitates-economy
title: The fake grid's money is a stock OpenSim region's on either flavour
topic: test
status: done
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 2
refs:
  [
    test-fake-grid-object-asset-id-divergence,
    test-fake-grid-imitates-sl-new-file-upload-announcement,
  ]
---

Done 2026-09-08. See "What landed" below.

Context: [context/testing.md](../context/testing.md).

`EconomyConfig` is already a builder knob, so unlike the other three
divergences this one needs no new behaviour to become flavour-decided — it
needs a **measurement**.

`stock_prices` is documented as the zeroes a stock OpenSim region answers with
(`SampleMoneyModule`), and that is what both flavours answer today. What Second
Life's `EconomyDataRequest` actually returns — the upload charges, the group
creation fee, the object-count limits — is not written down anywhere in this
workspace, so a Second-Life-flavoured default cannot be written honestly yet.

Two steps, in order:

- Measure it. A conformance case already logs into aditi; the reply is one
  `EconomyData` message and `economy-data` is registered on both live grids
  already, so what is missing is a record of the aditi numbers rather than new
  machinery.
- Then `ImitatedGrid` picks the price list, and the currency symbol with it
  (`L$` on both, but the helper flow behind it is not the same shape).

Worth keeping small: nothing a viewer *does* depends on these numbers being
right, only what it *displays*. It is on the list because a grid that says it
is Second Life and quotes a stock OpenSim region's zeroes is the same category
of lie as the other three, not because anything is blocked on it.

Acceptance: the aditi `EconomyData` numbers are recorded, `ImitatedGrid` picks
the price list, and `economy-data` asserts the flavour's own answer rather than
whichever one happened to be the default.

## What landed

The measurement, the derivation — and a decoder fix nobody had asked for,
because the honest OpenSim answer turned out to be one this workspace's client
could not receive.

**Two premises in the item above were wrong**, and the second is the whole
reason this cost more than two points:

- `stock_prices` was *not* "the zeroes a stock OpenSim region answers with". It
  was a synthetic table of seventeen made-up amounts, every L$ price a
  different number so a test could catch the encoder writing a price into the
  wrong slot. Both flavours answered it. So neither flavour was quoting a real
  grid, not just the wrong one.
- A stock OpenSim region does not answer zeroes either, and the reason is
  better than "five fields happen to differ". OpenSim's money module once
  shipped a near-verbatim copy of a Linden simulator's price list: its
  pre-2018 defaults were `100`, `10`, `4`, `4`, `1`, `1.0`, `5`, `2`, `2.0`,
  `1`, `1.0`, `10`, `1` — **thirteen of fifteen identical to what aditi
  answered in 2026**, the exceptions being the upload charge and the group
  price. A 2018 commit then zeroed most of them "to no cost values, since that
  is our default", and the five fields where the two grids still agree
  (`PricePublicObjectDecay`, `PriceParcelClaimFactor`,
  `TeleportPriceExponent`, `EnergyEfficiency`, `PriceObjectScaleFactor`) are
  simply what that commit left behind. The agreement is history, not policy.

**The measurement.** One aditi run of `economy-data`, after teaching the case to
record all seventeen fields rather than six headline ones — a field left
unrecorded is a field the fake grid has to invent. The numbers are in the
`economy_policy` module docs, the book chapter and `second_life_prices`.
`price_upload = 10` is the one
[[test-fake-grid-imitates-sl-new-file-upload-announcement]] was waiting on.

The OpenSim column is read off `SampleMoneyModule.ReadConfigAndPopulate` rather
than measured, and that is deliberate: the local grid's `bin/OpenSim.ini`
overrides `PriceUpload = 7` and `PriceGroupCreate = 11` on purpose, so a live
round trip is observable. Its `economy-data` run therefore *confirms* fifteen of
the seventeen and differs from stock in exactly the two that were overridden —
which is a better outcome than a grid configured to agree with the source would
have given.

**The decoder fix, which was not optional.** A stock OpenSim region sends `-1`
for `PriceGroupCreate` — `SampleMoneyModule` both initialises the field and
reads its config key with `-1` — and `linden_from_wire` rejected that as an
out-of-range L$ amount, dropping the **whole reply**. So a viewer against an
unconfigured OpenSim grid learned no price at all rather than sixteen prices and
one blank, and the fake grid could not have served a stock-accurate OpenSim list
even if asked to. `EconomyData::price_group_create` is an `Option<LindenAmount>`
now (`unpriced_linden_from_wire` / `unpriced_linden_to_wire`), `None` for any
negative. The other price fields keep the strict decode: no simulator has been
measured sending a negative for one of those, so a negative there is still a
malformed message worth dropping.

Nobody had hit this because the only OpenSim grid this workspace talks to
configures the field away.

**`None` there means "no price stated", not "free"** — and the `-1` is not a
considered sentinel, which is worth writing down because it reads like one. As a
price it is nonsense: `0` says free and is what all fourteen sibling prices use.
Three things explain it and none is intent. It is the value the reference
viewer's `LLBaseEconomy` initialises *every* price to before a reply arrives,
meaning "not received yet", and it reached OpenSim's config default from there.
It sits in a field nothing spends: OpenSim charges group creation from
`IMoneyModule.GroupCreationCharge`, hard-coded `0` in `SampleMoneyModule` and
guarded with `if (charge > 0)`, and never consults this number — while the
modern reference viewer prices group creation from the account's benefits
package. And the same 2018 commit quoted above as setting "no cost values" is
the one that moved this field's initialiser from `0` **to** `-1` while moving
all fourteen others *to* `0`. The author meant free; this is the one field that
spells free differently from its neighbours.

**A gap this exposed**, filed as [[protocol-account-benefits-package]]: on
Second Life the reference viewer does not price uploads from `EconomyData` at
all. `LLAgentBenefits` reads `texture_upload_cost` (and the tiered
`large_texture_upload_cost`, which applies above 1024×1024) out of the login
response's `account_level_benefits`, falling back to `EconomyData.price_upload`
only *off* Second Life. The same package is where Premium/Premium Plus pricing
and limits live, keyed by `account_type` with `premium_packages` listing them
all. `sl-wire` already carries all three fields — as opaque `Llsd` blobs that
nothing decodes. So the `price_upload = 10` measured here is a real measurement
of the legacy field and **not** what Second Life charges for an upload, which
matters directly to
[[test-fake-grid-imitates-sl-new-file-upload-announcement]]'s
`expected_upload_cost`; that item now says so.

**The derivation.** `ImitatedGrid::prices()` picks between `second_life_prices`
and `open_sim_prices`; `EconomyConfig::for_grid` builds the whole economy for a
flavour and `EconomyConfig::default` is now that for Second Life. The builder's
`economy` field became an `Option`, so it falls back to the flavour like every
other derived knob — with the footgun written down in
`FakeGridBuilder::economy`'s docs, because `EconomyConfig { site_valid: false,
..Default::default() }` on an OpenSim-flavoured grid would quietly hand it
Second Life's prices.

**The currency symbol is derived too**, and it is a divergence of *presence*
rather than of value. A stock OpenSim region announces no symbol anywhere:
`LLLoginResponse` defaults `currency` to the empty string and emits the key only
`if (currency != String.Empty)`, and its `OpenSimExtras` block carries
`currency-base-uri` without a symbol beside it. A viewer therefore falls back to
its own default — `OS$` in Firestorm, not `L$`. So
`ImitatedGrid::currency_symbol` is `Some("L$")` against `None`, and
`EconomyConfig::currency_symbol` became an `Option<String>` that the login
response and the extras block both follow.

Modelling absence rather than a second symbol is deliberate: on OpenSim the
symbol is a *deployment's* choice, not the software's. `StandaloneCommon.ini`
ships `Currency = ""` under "Ask co-operative viewers to use a different
currency name", real grids set it to their own, and Firestorm carries a
multi-currency subsystem that re-renders its UI when a region's extras override
the symbol mid-session. Picking one symbol for "OpenSim" would model one
deployment instead of the software and would hide the fallback path.

(This was very nearly got wrong. The first pass through this item declined to
derive the symbol on the grounds that it was `L$` on both grids and deriving it
would invent a difference — which was an assumption dressed as a measurement,
and wrong in both directions: not measured, and not one symbol.)

**What the new assertion does and does not catch**, verified by perturbation
rather than assumed. `economy-data` now runs on both fake flavours and compares
the whole reply against `ImitatedGrid::prices()`. Perturbing the *wiring* (a
`for_grid` that always returns Second Life's list) fails it, naming both tables
— so "a fake grid quotes the grid it says it is" is a test. Perturbing a *price*
does **not** fail it: both sides read the same constant. The encoder-slot check
the synthetic table used to provide therefore stayed where it belongs, in
`sl-proto`'s `send_economy_data` round trip over an all-distinct fixture; the
case docs say so rather than claiming a coverage it does not have.

`imitates.rs` no longer has a "what it does not decide yet" table. The economy
was the last entry in it.
