---
id: gridspec-lsl-money-permissions
title: LSL library behaviour on each grid — script permissions, money and payment
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-money-permissions, viewer-money-economy-ui,
  server-fake-grid-script-engine-wiring]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: When permissions lapse (state change, leaving the region), debit
behaviour, pay price. Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Script permissions, money and payment.

## Fake grid

Large — [[server-lsl-lib-money-permissions]].

## Viewer

Pay dialog with script prices.

## Done (2026-10-26)

- **Discover.** Four probes, `sl-lsl-runtime/probes/money/`: `pay-price`
  (what `RequestPayPrice` answers after each kind of `llSetPayPrice`, from a
  prim with a `money` event, one without and one given buttons of other
  types, and across a reset); `pay` (two scripts with a `money` event in one
  prim, paid by both avatars under four pay layouts, from a state with no
  `money` event and once none is left, and the payer refunded by
  `llGiveMoney`); `debit` (`llGiveMoney` and `llTransferLindenDollars`
  before and after `PERMISSION_DEBIT`, to odd destinations and of odd
  amounts, `transaction_result`, `PERMISSION_DEBIT` asked of a non-owner);
  `lapse` (a grant across a state change, the three revocations a viewer
  sends, `llReleaseControls`, the owner away, a reset, a take and rez, an
  attach and a drop). Complete on aditi and both OpenSim engines; every run
  nets both accounts to zero.
- **Runner.** Three drives: `pay` (a `MoneyTransferRequest` of type `5008`
  to a prim), `pay-price` (`RequestPayPrice`, with the reply noted) and
  `revoke` (`RevokePermissions` of prim A or the region); and
  `# money-balance` notes, a balance reply as the change it shows, its
  transaction's parties as roles.
- **Found on the way.** OpenSim answers no `RequestPayPrice` for a layout of
  zeros, pads `llSetPayPrice`'s list argument in place, lets a non-owner
  grant `PERMISSION_DEBIT`, and reports success for any transfer; Second
  Life checks a payment's amount only when the price field is hidden, and
  drops one without a word when no button matches; XEngine resets a script
  on take and rez. OpenSim takes any text for a true key. All in
  `book/src/gridspec/lsl.md` and `book/src/tools/lsl-probes.md`.
- **Document.** `book/src/gridspec/lsl.md` § Script permissions, money and
  payment, linked from `content/scripts.md`.
- **Fake grid.** Large, as planned: the measured model is noted on
  [[server-lsl-lib-money-permissions]]; which OpenSim engine `FakeOpensim`
  runs is a decision noted on [[server-fake-grid-script-engine-wiring]].
- **Viewer.** The `PERMISSION_DEBIT` question already raises the
  money-caution card and type-`5009` balance replies raise the payment
  notices. There is no pay dialog yet: what it must do with each grid's
  `PayPriceReply` and which amounts each grid takes is noted on
  [[viewer-money-economy-ui]].
