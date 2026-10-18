---
id: gridspec-money
title: Balance, L$ transfers, pay dialogs and buying objects on each grid
topic: gridspec
status: done
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

## Done (2026-10-18)

The `money` census (2av, live grids) asks each avatar for its balance, has A
give B L$ 1 and back, L$ 0, itself, an id that is nobody's and B more than it
holds; B asks a cube of A's for its pay buttons and pays it, plain and with a
probe script that sets `llSetPayPrice` and reports `money`; and B buys A's
cubes — not for sale, a copy whose next owner may not copy, a copy for L$ 0
and L$ 1, at the wrong price and kind (for L$ 0 and for more), the contents,
one content item (`BuyObjectInventory`) and the original for L$ 0 and L$ 1.
It squares B's balance at the end (`book/src/gridspec/economy.md`).
Findings:

- Second Life answers every gift, payment and sale with a balance reply to
  each side carrying a `TransactionInfo` block and its own description,
  takes a payment to an object with no script, rewrites a gift to an object
  as type `5008`, drops L$ 0 gifts silently, fails a gift to nobody or beyond
  the balance in a failed reply, fills `PAY_DEFAULT` buttons with 1, 5, 10,
  20, refuses a stale price or sale type in an alert, ignores a buy of
  something not for sale, and refuses `BuyObjectInventory`.
- OpenSim's stock money module reports balance 0, drops every transfer
  without a word, sends the pay buttons as set, refuses any price but L$ 0
  with an `IM_MESSAGEBOX` from nobody, and at L$ 0 refuses in "/"-agent
  alerts and sells without a balance reply; bought contents arrive as unasked
  `InventoryDescendents`.
- Both sell a copy whose next owner may not copy.

Fake grid: large — [[server-fake-grid-money-ledger]], now ready, with the
per-flavour behaviour written into it.

Viewer: the status bar shows the grid's currency symbol (`L$`, `OS$` on
OpenSim, or the grid's own; `sl-viewer-kit` `grid_currency`, `LoginAccount`
now carries the login's `currency`); a balance reply with a transaction
raises the reference's `PaymentSent` / `PaymentReceived` / `PaymentFailure`
once the other side's name is known (`sl-viewer-notices` `payment_notices`);
an `IM_MESSAGEBOX` raises the `SystemMessageTip` (`system_messages`). An e2e
test holds all three on both fake flavours (`e2e_windows`
`money_is_shown_and_told_as_each_grid_tells_it`). The pay, buy and history
floaters remain [[viewer-money-economy-ui]] and
[[viewer-object-pie-buy-take-chain]], both given the findings.
