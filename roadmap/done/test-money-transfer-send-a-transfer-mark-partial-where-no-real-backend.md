---
id: test-money-transfer
title: send a transfer (mark partial where no real backend)
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 15 — Money & economy `[both]`
---

Context: [context/test.md](../context/test.md).

`money-transfer` — send a transfer (mark partial where no real backend).
`2av`.

The primary gifts the secondary 1 L$ and asserts the payer's
`MoneyBalanceReply` echoes it (a `Gift` transaction from primary to secondary
for the amount sent); the secondary gifts it back for run neutrality. On
OpenSim the stock `BetaGridLikeMoneyModule`'s `MoneyTransferAction` is an empty
method, so no echo arrives and the run is marked partial. Passes green on both
grids: partial on OpenSim, complete on Aditi (real backend).
