---
id: viewer-money-economy-ui
title: Money / economy / L$ UI
topic: viewer
status: ready
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

## Audit 2026-10-15

Already in place: the **L$ balance** read-out in the status bar
(`sl-client-bevy-viewer/src/status_bar.rs`, `AgentBalance`, requested with
`Command::RequestMoneyBalance`), and a plain pay-an-avatar amount row in the
avatar profile (`sl-viewer-people/src/avatar_profile.rs`, sending
`Command::SendMoneyTransfer`). Nothing else listed here exists yet: no pay
dialog (and no viewer consumer of `PayPriceReply`), no buy-object /
buy-contents / buy-land / buy-currency floaters, no transaction history or
money tracker, and the OpenSim `currency` symbol is decoded but no viewer
label reads it.

## From gridspec-land-transactions (2026-10-17)

Where the helper scripts live is resolved by
`sl_wire::economy_helper_uri` in the reference's precedence: a region's
`OpenSimExtras.currency-base-uri`, then the grid's `get_grid_info`
`economy`/`helperuri`, then the built-in Second Life helpers by login host;
`sl_wire::helper_script_url` joins a script to it. The buy-land floater
should take its URL from there. Measured answers to `preflightBuyLandPrep`:
OpenSim's money module answers success with `landuse` (lower case — the
reference reads `landUse`, so its upgrade flag is never seen) and an empty
`membership`; aditi's helper fails every preflight with "Our billing system
is currently down. Please try again later." (2026-10-17), which the
reference shows as the floater's error and buys nothing. Free land needs no
L$ prompt: the reference sends the `ParcelBuy` straight away when no
membership or land-use upgrade is needed, no currency has to be bought and
`confirm` is not `password` (`LLFloaterBuyLandUI::runWebSitePrep`).
`book/src/gridspec/land.md` § Transactions.

## From gridspec-money (2026-10-18)

Now in place: the grid's currency symbol (`sl-viewer-kit` `grid_currency`,
the reference's login `currency` → `OS$` on OpenSim → region
`OpenSimExtras.currency` order), read by the status-bar balance; the
`PaymentSent` / `PaymentReceived` / `PaymentFailure` notices from a balance
reply's `TransactionInfo` block (`sl-viewer-notices` `payment_notices`); and
OpenSim's `IM_MESSAGEBOX` refusals as the `SystemMessageTip`. Every other
`L$` in the locale bundle is still literal: the reference rewrites them all
through `Tea::wrapCurrency`, so the pay, buy and history floaters should take
the symbol from `GridCurrency` as they are built. For the pay floater:
Second Life answers `RequestPayPrice` with its default amounts (1, 5, 10, 20)
already in place of `PAY_DEFAULT`, OpenSim with the raw `-2`, and the
reference shows a quick-pay button only for an amount above zero
(`LLFloaterPay::processPayPriceReply`). `book/src/gridspec/economy.md`.
