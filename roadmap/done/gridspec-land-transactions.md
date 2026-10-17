---
id: gridspec-land-transactions
title: Buying, selling, deeding, abandoning land and passes on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-land-transactions, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim's `SampleMoneyModule` validates every land buy and transfers nothing; SL
unmeasured; the fake grid flips ownership for free.

## Discover

OpenSim with estate owner and a second avatar; aditi needs a real purchase (the
user's call); landtool helper calls via `sl-repl`.

## Document

`book/src/gridspec/land.md` § Transactions.

## Fake grid

Small in this task: passes, overlay re-send after ownership change. Charging:
[[server-fake-grid-money-ledger]].

## Viewer

Helper URLs per grid; no L$ prompts for free land.

## Left for here by [[gridspec-parcel-info-dwell]] (2026-10-05)

A listing's group-owned bit (`0x04`) is measured on aditi and only read from
source on OpenSim. Once this task deeds the local parcel to a group, read its
`ParcelInfoReply` too — after leaving it alone for 45 s, since OpenSim caches
a listing for 30 s past its last read.

## Done (2026-10-17)

The census `land-transactions` (2av) measures refusals, the economy helper,
sales to a named buyer, passes, a deed and its listing, abandon and reclaim,
recording what each avatar is told and what the plot reads after, and holds
both grids to it with `Measured` constants. Written up in
`book/src/gridspec/land.md` § Transactions.

- **OpenSim**, everything, as the estate owner on a corner it divides off the
  Default Region. Refusals are silent; land changes push the record to every
  avatar in the region (`-10000` to those not on it) and re-send the overlay;
  a buy takes the group the `ParcelBuy` names (the reference sends the
  buyer's active group); abandoned land stays leased; a pass needs the pass
  list on, and a second one is refused with an agent alert.
- **Second Life**, the refusals and the helper only. The test avatars cannot
  get land on aditi (all land for sale is in closed Linden regions but a
  Cale rental parcel, and aditi's land helper answers "Our billing system is
  currently down"); see [[gridspec-aditi-test-land]]. Buying land not for
  sale and deeding are refused with alerts, abandoning and reclaiming
  silently; a pass to a parcel with a price but the pass list off **is sold**
  (L$ 10 a run to the aditi sandbox's group).

Fake grid: `ParcelPolicy` rows for the push reach, the refusal alerts,
abandon's status, passes needing the list and the pass-sale alert;
`sl-fake-grid/src/land_sales.rs` validates a buy (for sale, to the buyer, at
the price), lets only the holder deed or abandon and only the estate reclaim,
sells passes onto the allow list, and every change of hands re-sends the
overlay to the whole region. The census cannot run offline until the fake
grid divides parcels ([[server-fake-grid-parcel-divide-join]]); charging is
[[server-fake-grid-money-ledger]].

Viewer: `sl_wire::economy_helper_uri` resolves the helper per grid (region
`currency-base-uri`, `get_grid_info`, built-in Second Life helpers), for the
buy-land floater still to come ([[viewer-money-economy-ui]]); an alert that
begins `/` (OpenSim's non-modal agent alerts) is shown without the slash as a
system message, as the reference does.
