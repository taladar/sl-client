---
id: test-presence-online-offline
title: observe OnlineNotification / OfflineNotification as the peer logs in/o
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 4 — Friends & presence `[both] 2av`
---

Context: [context/test.md](../context/test.md).

`presence-online-offline` — observe `OnlineNotification` /
`OfflineNotification` as the peer logs in/out. `2av` (OpenSim now; Aditi
deferred → Phase Z). Presence flows over `OnlineNotification` /
`OfflineNotification`, which the grid's friends service sends only to friends
granted the see-online right; OpenSim grants `CanSeeOnline` in *both*
directions on a fresh friendship (`FriendsModule.AddFriendship`), so a clean
friendship is the only rights setup needed. Both avatars are already logged in
when a case starts, so the case drives the transitions via the mid-run
logout/login support `offline-msg-fetch` introduced: it first establishes a
clean friendship (pre-clean, offer, accept, confirm the grid's
`FriendshipAccepted`), then the secondary `disconnect`s and the primary — a
see-online friend — observes `Event::FriendsOffline` naming it
(`StatusChange(_, false)` from OpenSim's `OnClientClosed`), then the secondary
`relogin`s (inheriting the "already logged in" retry that evicts the stale
presence) and the primary observes `Event::FriendsOnline` naming it
(`StatusChange(_, true)`, fired once the returning agent is a root agent).
Each observation matches the secondary's id inside the notification's id list
so an unrelated friend's presence change cannot satisfy it. Where
`friendship-offer-accept` proves the friendship forms, this proves the
presence channel it opens carries both edges of the transition. The offline
notification is emitted as the grid tears the circuit down (inside the
disconnect's logout sequence), so it is already buffered by the time the
primary looks — observed near-instantly (≈ 0.04 ms); the online notification
follows the relogin at ≈ 84 ms loopback. `[opensim]` only.
