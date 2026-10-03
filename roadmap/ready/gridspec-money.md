---
id: gridspec-money
title: Balance, L$ transfers, pay dialogs and buying objects on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-money-economy-ui,
  viewer-object-pie-buy-take-chain]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim reports balance 0, refuses non-zero `ObjectBuy` with a BlueBox; SL
transfers complete; the fake grid has no ledger.

## Discover

`money-*` cases plus new buy / pay-price cases (for-sale and pay-script prims;
aditi sandbox with beta L$).

## Document

`book/src/gridspec/economy.md`.

## Fake grid

Large — [[server-fake-grid-money-ledger]].

## Viewer

OpenSim BlueBox refusal, balance 0, `OS$` fallback.
