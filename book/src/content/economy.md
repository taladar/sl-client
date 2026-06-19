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
