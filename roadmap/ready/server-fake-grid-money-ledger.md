---
id: server-fake-grid-money-ledger
title: Fake grid — a money ledger: balances, transfers, buying and paying
topic: server
status: ready
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

## From gridspec-money (2026-10-18)

The `money` census measures what to build (`book/src/gridspec/economy.md`);
add `FakeSl` and `FakeOpensim` to its grids once the ledger answers. The fake
grid now answers only `MoneyBalanceRequest` (the session's balance) and
nothing else of it: no `RequestPayPrice`, `MoneyTransferRequest`, `ObjectBuy`
or `BuyObjectInventory` handler. Per flavour:

- **Second Life**: a gift, a payment to an object (with a script or none —
  type `5001` to an object is taken as `5008`) and a purchase each send the
  payer and the payee a `MoneyBalanceReply` with a fresh transaction id and a
  `TransactionInfo` block, the payee's own description ("*payer* paid you
  L$1.", "… for Object."; the seller's is empty at L$ 0); L$ 0 gifts are
  dropped silently; a gift to oneself is two replies; to an unknown id a
  failed reply "There has been a problem transferring L$. This error has been
  logged and reported."; beyond the balance a failed "Insufficient funds.".
  A paid script's `money` event fires. `PayPriceReply` puts 1, 5, 10, 20 in
  for `PAY_DEFAULT`. A purchase refuses a wrong price or type with an alert
  (`Sale of Object cancelled.  Price does not match expected.` /
  `…  Sell type (copy,original,contents) does not match expected.`), says
  nothing to a buy of something not for sale, sells an original with an alert
  keyed `NowOwnObject`, contents as a `BulkUpdateInventory` folder in the
  named folder, and refuses `BuyObjectInventory` (`InventoryNotForSale`).
- **OpenSim** (stock `BetaGridLikeMoneyModule`): balance 0, every transfer
  dropped without a word, `PayPriceReply` as the script set it; a purchase
  for any price but L$ 0 refused with an `IM_MESSAGEBOX` from nobody
  ("Buying anything for a price other than zero is not implemented"), at
  L$ 0 the agent alerts "/Object <name> is not for sale", "… price does not
  match selected price", "… sell type does not match selected type"; a sold
  copy is an `UpdateCreateInventoryItem`, contents unasked
  `InventoryDescendents` of the root and the new folder; no balance replies;
  `BuyObjectInventory` unhandled.
