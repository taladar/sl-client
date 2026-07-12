---
id: test-grant-user-rights
title: grant see-online / map / modify rights; confirm
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 4 — Friends & presence `[both] 2av`
---

Context: [context/test.md](../context/test.md).

`grant-user-rights` — grant see-online / map / modify rights; confirm.
`2av` (OpenSim now; Aditi deferred → Phase Z). A friendship is born with only
`CAN_SEE_ONLINE` granted both ways; a client raises a friend's rights with
`GrantUserRights`. OpenSim's `FriendsModule.GrantRights` persists the new
bitfield then **always echoes it to the grantor**
(`SendChangeUserRights(requester, friend, rights)`) and notifies the friend
(`LocalGrantRights` → `SendChangeUserRights(requester, friend, rights)`); the
two `ChangeUserRights` packets carry the same `AgentData.AgentID` (the
grantor), so the session tells them apart by direction — the grantor sees its
own id (`granted_to_us = false`, updating `rights_granted`), the friend sees a
foreign id (`granted_to_us = true`, updating `rights_received`). The case
forms a clean friendship (the offer/accept flow), then the primary grants the
secondary the full `CAN_SEE_ONLINE | CAN_SEE_ON_MAP | CAN_MODIFY_OBJECTS` set,
both sessions observe the matching `Event::FriendRightsChanged`, and a
`QueryFriends` on each side confirms the cached friendship now reflects it
(primary's `rights_granted` to the secondary is the full set; secondary's
`rights_received` from the primary is the full set; the reverse direction
stays at the default). Surfaced a grid-side timing dependency: `GrantRights`
only acts on a friend present in the *grantor's* server-side friends cache,
which `RecacheFriends` refreshes asynchronously and races the
`FriendshipAccepted` IM — granting the instant the IM lands finds the cache
still empty and echoes nothing, so the case settles ~3 s after the accept
before granting (`GRANT_SETTLE`). Keeping the see-online bit set means the
grant toggles no presence, so it provokes no spurious online/offline
notification. Green on OpenSim; echo RTT ≈ 4.6 ms, notify RTT ≈ 4.7 ms
loopback. `[opensim]` only.
