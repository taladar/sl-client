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
