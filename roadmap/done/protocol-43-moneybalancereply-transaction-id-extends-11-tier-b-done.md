---
id: protocol-43
title: MoneyBalanceReply transaction id (extends #11, Tier B). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**43. `MoneyBalanceReply` transaction id (extends #11, Tier B). ✅ Done.**
`money_balance` (`session.rs`) dropped `MoneyData.TransactionID`, so a balance
reply (and its `MoneyTransaction` description) couldn't be correlated back to
the pay/buy that triggered it. Surfaced it as a new **`transaction_id: Uuid`**
field on the `MoneyBalance` value, populated from `data.transaction_id`. It is
the id the simulator echoes from the triggering transaction — e.g. the
`TransactionID` a `Session::send_money_transfer` carried — so a client tracking
in-flight payments can match the resulting (often unsolicited) balance reply to
the pay/buy that caused it; it is nil for a plain balance poll, which has no
triggering transaction. The field flows through both runtimes unchanged (the
event is a shared `sl-proto` type and every consumer binds `MoneyBalance(_)`, so
no command wiring was needed). Covered by the two existing `sl-proto` lifecycle
tests, extended to assert the new field: `money_balance_reply_surfaces_balance`
(a plain poll → nil `transaction_id`) and
`money_balance_reply_surfaces_transaction_details` (a real payment → the
`TransactionID` round-trips alongside the `MoneyTransaction` details).
*Unit-tested only: on a plain balance poll OpenSim sends a nil `TransactionID`,
and its `BetaGridLikeMoneyModule` routes no real transactions (the same reason
the #11 transfer path is unit-tested), so the non-nil correlation case — the
point of this fix — needs a money backend (Gloebit/DTL) or the real SL grid.
Test: money module or SL grid.*
