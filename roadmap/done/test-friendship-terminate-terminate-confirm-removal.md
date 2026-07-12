---
id: test-friendship-terminate
title: terminate, confirm removal
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 4 — Friends & presence `[both] 2av`
---

Context: [context/test.md](../context/test.md).

`friendship-terminate` — terminate, confirm removal.
`2av` (OpenSim now; Aditi deferred → Phase Z). The case first forms a clean
friendship (the `friendship-offer-accept` flow: pre-clean, offer, accept,
confirm both buddy lists) so there is a real friendship to tear down, then the
primary `TerminateFriendship`s the secondary. `TerminateFriendship` names the
former friend (`ExBlock.OtherID`); OpenSim's `RemoveFriendship` deletes the
symmetric record and sends a `TerminateFriendship` back to *both* parties —
`client.SendTerminateFriend` echoing the removal to the terminator, plus
`LocalFriendshipTerminated` → `friendClient.SendTerminateFriend` informing the
dropped friend. Each side's `Session` surfaces this as
`Event::FriendshipTerminated` and drops the peer from its buddy cache (the
terminator does *not* remove locally on send — it relies on the grid echo).
The case asserts the primary observes its own `FriendshipTerminated` (naming
the secondary), the secondary observes the matching one (naming the primary),
and a follow-up `QueryFriends` on each side reports the other gone. Green on
OpenSim; echo RTT ≈ 13 ms, notify RTT ≈ 13 ms loopback. `[opensim]` only.
