---
id: server-fake-grid-money-ledger
title: Fake grid — a money ledger: balances, transfers, buying and paying
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-money, gridspec-land-transactions]
---

Context: [context/server.md](../context/server.md).

## What

No ledger exists. Balances, transfers, buy object / contents / copy, pay price,
land charges; OpenSim's zero balance and free-only buys as flavour rows.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## From gridspec-land-transactions (2026-10-17)

Second Life charges a pass: aditi's sandbox sold one for L$ 10 and answered
the buyer with a `MoneyBalanceReply` of transaction type `5006`
(`LandPassSale`), buyer to the parcel's group, "You paid <group> L$10 for a
land access pass." — and sold a second one while the first still had hours
left. OpenSim's pass and land sales go through `IMoneyModule.MoveMoney`,
which the local `BetaGridLikeMoneyModule` makes a no-op: nothing is charged
and no balance reply is sent. The fake grid sells both for nothing on either
flavour until this ledger exists. `book/src/gridspec/land.md` §
Transactions.
