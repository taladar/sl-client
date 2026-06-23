# Economy & Money

The grid has an in-world currency (L$ on Second Life) and a small economy
protocol around it: querying your **balance**, learning the grid's **prices**,
and **transferring** money to avatars and objects.

## Balance and prices

- **Money balance** — the avatar's current currency balance, returned with each
  balance request and after any transaction that changes it. Beyond the raw
  balance it can carry land-use accounting (square metres credited / committed).
  Request it with `Command::RequestMoneyBalance`; receive `Event::MoneyBalance`.
- **Economy data** — the grid's price list and limits: upload fees, parcel claim
  price, object/group creation fees, and per-region object capacity. Request
  with `Command::RequestEconomyData`; receive `Event::EconomyData`. A client
  needs this to show prices before an action.

## Transactions

Money moves via `Command::SendMoneyTransfer { to_agent_id, amount, type, … }`,
where the **transaction type** records *why* the money moved — a gift between
avatars, a payment to an object, an object sale, and so on. The transaction is
echoed back, and a fresh `Event::MoneyBalance` reflects the new balance.

> **Testing note.** The economy needs a real currency backend. On the OpenSim
> test grid, money support is off by default; enabling the demo money module
> lets the messages flow, but balances are effectively hardcoded (often `0`) and
> there is no real settlement — enough to exercise the protocol, not to model
> real spending.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/economy.rs`: `MoneyBalance`,
>   `MoneyTransaction`, `MoneyTransactionType`, `EconomyData` (amounts use the
>   `LindenAmount` type).
> - Commands `RequestMoneyBalance`, `RequestEconomyData`, `SendMoneyTransfer`
>   are in `sl-proto/src/command.rs`; events `MoneyBalance`, `EconomyData` are
>   in `sl-proto/src/types/event.rs`.
> - Every non-negative L$ *price/fee* field across the protocol is typed
>   `LindenAmount` too: the `EconomyData` price list; object/inventory
>   `sale_price` and `ownership_cost`; the parcel `sale_price`, `claim_price`,
>   `rent_price`, `pass_price`, and per-metre land price; the classified
>   `price_for_listing`; the group `membership_fee`; the `GroupAccountSummary`
>   tax/fee/credit/debit totals; and the directory `PlacesResult.price`. The
>   codec boundary decodes them with `linden_from_wire`, which *rejects* a
>   negative wire value (one no conforming simulator ever sends) rather than
>   masking it to `0`, so a malformed price drops the message instead of being
>   silently misread.
> - The genuinely *signed* L$ fields — a group's current `balance` and the
>   signed `amount` of a group-accounting detail line or transaction (a credit
>   is positive, a debit negative) — use the dedicated `LindenBalance` type: a
>   sign plus a non-negative `LindenAmount` magnitude, with arithmetic that
>   composes balances and amounts by type (`LindenBalance + LindenAmount`,
>   `From<LindenAmount>`, and a fallible `LindenAmount: TryFrom<LindenBalance>`
>   that rejects a negative balance). Zero is canonically non-negative, so there
>   is no negative-zero. Like `LandArea`, `LindenBalance` is kept client-local
>   in `sl-proto` for now, slated to move to `sl-types` with the other value
>   types in one later update.
> - A *sale* price is an `Option<LindenAmount>`: `None` when the item/parcel is
>   not for sale (the for-sale state is the separate `sale_type` / `FOR_SALE`
>   flag / `for_sale` field) — a for-sale item may still be free
>   (`Some(LindenAmount(0))`). On the wire `None` is the `0` not-for-sale
>   sentinel. This covers `ObjectProperties`/`ObjectPropertiesFamily`,
>   `InventoryItem`, `RestoreItem`, `ParcelInfo`/`ParcelUpdate`/`ParcelDetails`,
>   `DirLandResult`, and `Command::SetObjectForSale`; `ObjectBuyItem`'s price is
>   a plain `LindenAmount` (the bid you must match).
> - Land *area* (square metres, distinct from L$) is its own `LandArea(u32)`
>   newtype — the group land `contribution`, a parcel's `area`/`actual_area`/
>   `billable_area`, and the avatar's `square_meters_credit`/`_committed` — so a
>   land area and an L$ amount can't be transposed. (Group `contribution` is
>   land area, *not* L$, despite the wire field name — the viewer renders it as
>   `[AREA]`.) `LandArea` is kept client-local in `sl-proto` for now, slated to
>   move to `sl-types` with the other value types in one later update.
