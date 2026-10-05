---
id: gridspec-land-transactions
title: Buying, selling, deeding, abandoning land and passes on each grid
topic: gridspec
status: ready
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
