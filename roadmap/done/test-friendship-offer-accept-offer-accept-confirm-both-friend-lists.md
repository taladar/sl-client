---
id: test-friendship-offer-accept
title: offer, accept, confirm both friend lists
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 4 — Friends & presence `[both] 2av`
---

Context: [context/test.md](../context/test.md).

`friendship-offer-accept` — offer, accept, confirm both friend lists.
`2av` (OpenSim now; Aditi deferred → Phase Z). A friendship offer is an
`ImprovedInstantMessage` with the `IM_FRIENDSHIP_OFFERED` dialog
(`ImDialog::FriendshipOffered`), routed by the grid's friends service to the
named recipient (not broadcast like local chat). The primary
`OfferFriendship`s the secondary, which — a separate session — observes the
matching `Event::InstantMessageReceived` (`FriendshipOffered`, attributed to
the primary) and answers with `AcceptFriendship` quoting the offer's
transaction id (the IM's `id`, which OpenSim sets to the offerer's agent id).
The grid stores the symmetric friendship and notifies the offerer with a
`FriendshipAccepted` IM, which the primary observes (and which adds the
secondary to its buddy cache); the accepter adds the offerer on its own
accept. The case then confirms both buddy lists via `QueryFriends` /
`Event::FriendsSnapshot`. OpenSim rejects an offer to an *existing* friend
outright ("This person is already your friend", forwarding nothing), so the
case pre-cleans any leftover friendship with a best-effort
`TerminateFriendship` (a no-op when not friends) plus a short settle before
offering, and terminates again at the end so re-runs start clean — verified
idempotent across back-to-back runs. OpenSim ignores the calling-card folder
in `AcceptFriendship`, so the case passes the nil folder. Green on OpenSim;
offer RTT ≈ 5–13 ms, accept RTT ≈ 10–27 ms loopback. `[opensim]` only.
