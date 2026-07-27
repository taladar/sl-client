---
id: server-economy-service
title: Economy service — balances and transactions
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

The money side, if the grid wants one: per-account balances, the
transaction ledger (pay agent/object, object sales with the permission
transfer, group liabilities/dividends, upload/classified fees),
`MoneyBalanceReply`/transaction-history backends, and the economy
parameters the client fetches (`EconomyDataRequest`).

Deliberately a service with a trait boundary so a grid can run without
money (balances hardcoded, transfers refused — the local OpenSim
`BetaGridLikeMoneyModule` behaviour) or with a real backend. Fraud/
atomicity concerns (a sale must transfer object, inventory, and money
atomically across three services) make this more design-sensitive than
its size suggests.
