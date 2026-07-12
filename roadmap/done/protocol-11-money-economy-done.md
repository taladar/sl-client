---
id: protocol-11
title: Money / economy (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier B
---

Context: [context/protocol.md](../context/protocol.md).

**11. Money / economy (done) ✅ — `MoneyBalanceRequest`/`Reply`,
`MoneyTransferRequest`, `EconomyData`/`Request` · 5 pts.** L$ balance and
transfers — a balance monitor or tip/vendor bot (stronger combined with #2/#8,
but a balance/transfer tool stands alone). `Session::request_money_balance` /
`request_economy_data` / `send_money_transfer` (the latter taking a
`MoneyTransactionType` — `Gift`, `PayObject`, `ObjectSale`, or `Other(i32)`);
replies surface as `Event::MoneyBalance` (balance as `sl_types::LindenAmount`,
plus the optional `TransactionInfo` as `MoneyTransaction` when the reply
describes a real payment) and `Event::EconomyData` (upload/claim/group prices,
region object capacity). Wired through both runtimes
(`Command::RequestMoneyBalance` / `RequestEconomyData` / `SendMoneyTransfer`).
*Live-verified against local OpenSim with `economymodule =
BetaGridLikeMoneyModule`: a `MoneyBalanceReply` (balance 0 L$, success) and
`EconomyData` (the configured upload/group-create prices) both round-tripped on
one login. Stock OpenSim's module hardcodes a 0 balance and does not route real
transfers, so the transfer path is unit-tested only; full transfers need a money
backend (Gloebit/DTL) or the real SL grid.*
