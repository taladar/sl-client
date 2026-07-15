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

Reference (Firestorm, read-only): `llfloaterpay`, `llfloaterbuycurrency(html)`,
`llfloaterbuyland`, `llstatusbar` (balance), `fsmoneytracker`,
`llmarketplacefunctions`.

Deps: [[viewer-ui-widget-scaffold]], [[viewer-media-prim-browser]] (HTML
currency / marketplace flows).
