---
id: test-group-accounting
title: account summary / details / transactions
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

`group-accounting` — account summary / details / transactions. `1av`.
The three requests a viewer issues for a group's "Land & L$" floater, one per
tab: `RequestGroupAccountSummary` (→ `Event::GroupAccountSummary`),
`RequestGroupAccountDetails` (→ `Event::GroupAccountDetails`), and
`RequestGroupAccountTransactions` (→ `Event::GroupAccountTransactions`). Each
is a reliable `S32`-parameterised request keyed by a client-chosen `RequestID`
echoed back for correlation; the case mints a fresh `GroupRequestId` per
request and pairs every reply by it, with interval parameters matching the
reference viewer exactly (summary 7-day / details 1-day / transactions 7-day,
all at current interval 0). The group comes from `support::membership_group`
(index 0) so the primary holds the group's `Accountable` power. Listed
`[both]`, but the grids exercise different halves. **OpenSim has no
group-accounting backend:** `LLClientView` parses and acks all three requests
and fires `OnGroupAccountSummaryRequest` and siblings, but no region module
subscribes to those events, so it never replies (the `SendGroupAccounting*`
methods exist but are dead code) — confirmed across core and optional modules
including `SampleMoneyModule` (the `BetaGridLikeMoneyModule` config target).
The OpenSim run therefore proves the client *encodes and transmits* all three
requests in a form a real simulator accepts — it watches the circuit past the
reliable-retransmit budget via keep-alive pings (the
acceptance-by-absence-of-failure check `throttle-set` uses) — then marks the
dataset partial, since no reply data is observable. Green-partial on OpenSim:
create ≈ 0.43 s, ping ≈ 0.5 ms loopback, 3 requests sent, 0 replies. The
**reply assertions are the Second Life variant** (deferred with the Aditi
batch): wait for all three replies, correlate each by request/group id, and
assert the echoed interval parameters; the SL run additionally needs the
primary to hold the configured pre-made group's `Accountable` power.
