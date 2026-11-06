---
id: server-fake-grid-money-ledger
title: Fake grid — a money ledger: balances, transfers, buying and paying
topic: server
status: done
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

## From server-fake-grid-script-engine-wiring (2026-11-02)

The script engine runs on the fake grid and its money calls reach the host,
which has nothing to move them with (`sl_fake_grid::scripts`, `RegionHost`):

- **`give_money` / `transfer_money`**: the owner pays the payee, transaction
  type `5009`, the object's name as the item, the two balance replies; a
  transfer the economy refuses is a `MoneyRefusal` (no account, or on Second
  Life insufficient funds with the failed balance reply to the owner).
  Today `llGiveMoney` moves nothing and `llTransferLindenDollars` refuses only
  a payee with no account.
- **A payment to an object** (`ServerEvent::MoneyTransferRequested`, type
  `5008`): judged by `PayPrice::takes` on Second Life (taken, dropped
  silently, or refused with `PAYMENT_STOPPED` as an alert to the payer); a
  taken one is moved and raised with `Engine::paid` in every script of the
  prim. `RequestPayPrice` is already answered from the layout the script set.
- **Acceptance**: a vendor prim paid raises `money` with the amount, and the
  payer's balance and the economy event stream agree; from the viewer once
  it has a pay dialog (`viewer-money-economy-ui`), from a conformance case's
  session until then.

## From server-world-changed-raisers (2026-11-03)

`events/changed-owner` (`book/src/gridspec/lsl.md` § `changed`: what a viewer's
change raises): a purchase of an original for L$ 0 raises `CHANGED_OWNER` in
the bought prim's script **once** on Second Life, the new owner read at once,
and **three times** on OpenSim (both engines). Second Life tells the buyer
`You are now the owner of object Object` and sends both a `MoneyBalanceReply`
of type 5000 ("You paid <seller> L$0 for Object."). Raise it with
`scripts::raise_changed_in` in every prim of the bought set when a purchase
lands (`changes.rs` holds the bit constants).

## Done (2026-11-06)

The grid keeps a ledger (`sl_fake_grid::money`, a `Ledger` on `GridCore`):
every account starts at `ImitatedGrid::upload_funds` (L$ 10 000 on Second
Life; none kept on OpenSim) and keeps what moved it for as long as the grid
runs. A per-session money responder answers `MoneyBalanceRequest` (moved out
of `groups.rs`), `MoneyTransferRequest`, `ObjectBuy`, `BuyObjectInventory` and
`ParcelBuyPass`; `ImitatedGrid::money_policy` holds what the ledger alone does
not say. A session's own `SimSession` balance is written from the ledger
before every capability request and its charge read back after, and a charged
upload is followed by the new balance, as Second Life does. A founding fee is
the ledger's too.

- **Gifts and payments**, Second Life: moved, both sides told with a fresh
  transaction id and the `TransactionInfo` block; to nobody and beyond the
  balance a failed reply to the payer; a payment to a prim taken as `5008`
  whatever type it named, judged by `PayPrice::takes` (dropped, or refused with
  `PAYMENT_STOPPED`), raising `money` in every script of the prim. OpenSim:
  dropped.
- **Pay buttons** of a prim no script set: the default layout, per flavour.
- **Purchases** as the census measured them, per flavour: Second Life sells at
  any price, refuses the wrong kind and then the wrong price by alert, tells
  both sides with type `5000` and the original's buyer `NowOwnObject`; OpenSim
  refuses any price but L$ 0 by message box and at L$ 0 not-for-sale, price and
  kind by agent alert. A copy is an item backed as a take of a copy is
  (`world::file_linkset_copy`), the contents a new folder (over the event queue
  on Second Life, as unasked descendents of the root and the folder on
  OpenSim), the original changes hands with the next owner's masks and raises
  `CHANGED_OWNER` once / three times. One item of the contents is refused with
  `InventoryNotForSale` on Second Life, unanswered on OpenSim.
- **Scripts**: `llGiveMoney` / `llTransferLindenDollars` pay out of the owner's
  balance, both told by type `5009` replies delivered after the tick (a new
  `Delivery::Balance`), a payee that is nobody's or beyond the balance refused
  with the failed reply; nothing moves on OpenSim.
- **A land pass** (now sold by the money responder rather than
  `parcel_edits`) is paid to the parcel's owner on Second Life, "You paid
  *owner* L$10 for a land access pass." in a type `5006` reply.

Held by the `money`, `money-balance` (both flavours) and `money-transfer`
(Second Life flavour) conformance cases, now offline; `upload-http` and
`asset-upload` now hold the fake grid to the balance and the charge too.
`tests/scripts.rs` adds `a_script_pays_out` and
`a_bought_original_tells_its_scripts` on both flavours, and
`tests/client_end_to_end.rs` `a_pass_is_paid_for_on_second_life`. Book:
`gridspec/economy.md`, `land.md`, `lsl.md` (the money tables and the
bought-original row) and `simulator/lsl-engine.md`.

Not done, each unmeasured or another task's:

- **A parcel's purchase is still free** on both flavours: aditi's land helper
  refuses every purchase, so Second Life's charge for land was never measured
  (`book/src/gridspec/land.md` § Transactions).
- **Money held by a group**: a gift to a group, or a payment to or purchase
  of a group's object, is refused as one to nobody; a pass to a group's parcel
  is paid to nobody. Unmeasured.
- **The viewer** has no pay dialog or buy floater yet
  (`viewer-money-economy-ui`); its payment notices, measured against aditi,
  read the same replies the fake grid now sends.
