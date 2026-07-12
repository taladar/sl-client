---
id: test-calling-card
title: offer/accept calling card
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 4 — Friends & presence `[both] 2av`
---

Context: [context/test.md](../context/test.md).

`calling-card` — offer/accept calling card. `2av` (OpenSim now, partial;
Aditi deferred → Phase Z). A calling card is a reference card to an avatar,
filed in the recipient's Calling Cards folder; offering one is *not* a
friendship request. The primary `OfferCallingCard`s the secondary with a fresh
correlation id; the secondary observes the matching
`Event::CallingCardOffered` attributed to the primary and
`AcceptCallingCard`s, quoting the offer's transaction id. Contrary to this
roadmap's earlier guess, OpenSim **does** surface the offer when both avatars
share a region: `XCallingCardModule.OnOfferCallingCard` finds the recipient
in-region, creates the calling-card inventory item, and pushes it with
`SendOfferCallingCard(from, itemID)` — so the secondary's `CallingCardOffered`
carries the *new card's item id* as its transaction (the in-region path
discards the offerer's chosen transaction entirely), and the case asserts the
offer is attributed to the primary rather than that the transaction
round-trips. The run is partial because OpenSim's `OnAcceptCallingCard` is an
empty no-op (the card was already filed at offer time), so it sends the
offerer **nothing** back — the offerer-side `Event::CallingCardAccepted`
confirmation has no OpenSim path to observe. The full
offer→accept→offerer-confirm round-trip is Second Life only → Phase Z (aditi).
Green-partial on OpenSim; offer RTT ≈ 62 ms loopback. `[opensim]` only.

- All Aditi variants deferred to Phase Z.
