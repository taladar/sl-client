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
transfers complete; the fake grid has no ledger between avatars. It does keep
one balance per session (`SimSession::money_balance` / `charge_money` in
`sl-proto/src/sim_session.rs`): debited by an upload's fee and a group's
founding fee, and stated in reply to `MoneyBalanceRequest`
(`sl-fake-grid/src/groups.rs`, `answer_balance`).

## Discover

`money-*` cases plus new buy / pay-price cases (for-sale and pay-script prims;
aditi sandbox with beta L$).

## Document

`book/src/gridspec/economy.md`.

## Fake grid

Large — [[server-fake-grid-money-ledger]].

## Viewer

OpenSim BlueBox refusal, balance 0, `OS$` fallback.
