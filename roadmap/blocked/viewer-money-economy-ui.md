---
id: viewer-money-economy-ui
title: Money / economy / L$ UI
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-widget-scaffold, viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

The economy surface: L$ balance display, the pay dialog, buy-object / buy-land /
buy-currency flows, a transaction history, and marketplace access. Some of these
(currency purchase, marketplace) are HTML flows that ride the embedded browser.

Explicit scope details (script-interface survey 2026-07-23):

- The pay dialog must run the `RequestPayPrice` / `PayPriceReply` round-trip
  (`llSetPayPrice`): render the script-defined default amount and up to four
  quick-pay buttons, or hide them for `PAY_HIDE`. The wire side is decoded
  (`Event::PayPriceReply`, [[api-g6]]).
- The per-session live money tracker (`fsmoneytracker`): running
  earned/spent totals with a compact floater, beyond the transaction
  history.

Reference (Firestorm, read-only): `llfloaterpay`, `llfloaterbuycurrency(html)`,
`llfloaterbuyland`, `llstatusbar` (balance), `fsmoneytracker`,
`llmarketplacefunctions`.

Deps: [[viewer-ui-widget-scaffold]], [[viewer-media-prim-browser]] (HTML
currency / marketplace flows).

## Parity-audit addendum (2026-08-19)

The floater-registry audit found three more registered money floaters
this task should absorb beyond buy-object/buy-land/buy-currency and the
FS money tracker: **Buy Contents** (`floater_buy_contents.xml` — buy the
contents of a for-sale object as a folder) and the **add-payment-method**
web floater (`floater_add_payment_method.xml`). The **Sell Land** flow
(`floater_sell_land.xml`) registered alongside them is NOT part of this
task: it moved to [[viewer-land-transactions]] together with the other
About Land General-tab land-transfer actions; only the Buy Land purchase
floater itself (currency estimation, covenant agree, group-contribution
removal) stays here.

Addition from the audit: OpenSim multi-currency support via
OpenSimExtras (`lfsimfeaturehandler.cpp`) — the `currency` symbol
replaces the "L$" label and `currency-base-uri` overrides the economy
helper URI per region, with a change notification when the region
switches either. Wire decode already exists in
`sl-wire/src/sim_features.rs`.
