---
id: protocol-6
title: Friends & presence
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**6. Friends & presence · 5 pts. ✅ Done.** A standalone presence/online
monitor. Implemented: the **friend list arrives at login** — the request now
asks for `buddy-list`, and the response parser extracts each friend's id plus
the two rights bitfields (`BuddyListEntry`), surfaced once as
`Event::FriendList(Vec<Friend>)` right after `CircuitEstablished`. **Presence**
is sim-pushed: `OnlineNotification` /`OfflineNotification` surface as
`Event::FriendsOnline`/`FriendsOffline` (`Vec<Uuid>`). **Rights**:
`Session::grant_user_rights` (`GrantUserRights`) sets the rights granted to a
friend, and incoming `ChangeUserRights` surfaces as
`Event::FriendRightsChanged { friend_id, rights, granted_to_us }` — the
`granted_to_us` flag distinguishes a friend changing their grant to us from the
sim echoing our own change (OpenSim's `AgentData.AgentID == self` hack).
**Friendship offer/accept via IM**: `Session::send_friendship_offer`
(`ImprovedInstantMessage` `IM_FRIENDSHIP_OFFERED`), plus
`accept_friendship`/`decline_friendship`
(`AcceptFriendship`/`DeclineFriendship`, echoing the offer IM's `id` as the
transaction id) and `terminate_friendship` (`TerminateFriendship`). A
`FriendRights` bitfield value type wraps the rights flags
(`CAN_SEE_ONLINE`/`CAN_SEE_ON_MAP`/`CAN_MODIFY_OBJECTS`). Wired as
`Command::{OfferFriendship, GrantUserRights, TerminateFriendship,
AcceptFriendship, DeclineFriendship}` through both runtimes; verified live
against the local OpenSim with two accounts
(offer→accept round-trip, friend list at re-login, and online/offline
notifications). *Test: local OpenSim with two accounts.*
