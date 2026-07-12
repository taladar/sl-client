---
id: chat-a3
title: Design the friend-presence state model
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A3. Design the friend-presence state model.** A buddy-list cache
(`Friend { id, rights_granted, rights_received }`) and an online set keyed by
`FriendKey`, seeded by `FriendList` at login and updated by `FriendsOnline` /
`FriendsOffline` and `FriendRightsChanged`. Presence is friends-only /
`CAN_SEE_ONLINE`-gated / passive. Drive the online set **only** from the
authoritative presence notifications (and the login buddy list) — never infer
presence from IM send/receive activity. (Known reference-viewer / SL-grid bug
to **avoid replicating**: an IM sent immediately after a peer goes offline
falsely re-marks them online; this design must ignore IM traffic as a presence
signal.) Accessors: `friends()`, `is_online(friend)`, `online_friends()`.
**Done — see § Friend-presence reference (from A3) + task B1 in § Phase B.**
Decided: two independent private fields —
`friends: BTreeMap<FriendKey, Friend>` (the buddy cache, the value's `id` ≡
the key) and `online: BTreeSet<FriendKey>`. `friends` is seeded from the
existing `Event::FriendList` build site (`methods.rs:1078`), mutated by
`FriendRightsChanged` (`granted_to_us` picks `rights_received` vs
`rights_granted`), and dropped by `FriendshipTerminated` (its doc already says
"drop `other`"). `online` is the **sole** truth — `OnlineNotification`
inserts, `OfflineNotification` removes, termination removes — and is **never**
touched by any IM handler (the invariant that dodges the
"IM-after-offline → falsely online" bug). The stores stay **independent** in
the presence sense (`online` is never inferred from `friends` or IM traffic),
**but `friends` is maintained live** — a friendship *formed mid-session* is
added the moment it forms, **not** deferred to relogin (the 2026-06-27
revision): the inbound `FriendshipAccepted` IM carries the new friend's
`from_agent_id` (they accepted our offer), and `accept_friendship` gains a
`friend_id: FriendKey` arg so the accepter side records it too — both insert a
`Friend` with the grid-default rights `CAN_SEE_ONLINE` in **both** directions
(grounded in OpenSim `StoreFriendships`; SL matches), reconciled by any later
`ChangeUserRights`. `FriendshipTerminated` drops the friend from both stores.
`is_online` = "known-online via a notification"; absence ≠ provably offline (a
friend who does not grant `CAN_SEE_ONLINE` never notifies). Accessors return
the public `Friend` (already `Copy`) directly.

## Friend-presence reference (from A3)

The buddy cache + online set folded in here. Presence is **friends-only
/ `CAN_SEE_ONLINE`-gated / passive** (the sim pushes it; there is no
`RequestOnlineNotification`) and **grid-level** (it persists across teleport —
A9). The simulator stays authoritative; these two stores are an API-convenience
read model, fed **only** by the authoritative friend signals, never inferred.

**Two independent fields** on `Session` (`session.rs`), beside the A2
`chat_sessions` and the `sit` / `teleport` enums, private, reached only through
accessors:

    friends: BTreeMap<FriendKey, Friend>   // buddy-list cache
    online:  BTreeSet<FriendKey>           // who is currently known-online

- **`friends`** keys by `FriendKey` → the existing public `Friend`
  (`types/avatar_profile.rs:316`, `#[derive(… Copy …)]`,
  `{ id, rights_granted, rights_received }`). Storing the whole `Friend` (whose
  `id` always equals the key — the invariant) lets `friends()` yield the public
  type with zero conversion, no new view struct. `BTreeMap` keeps the crate's
  deterministic iteration.
- **`online`** is a bare `BTreeSet<FriendKey>` — the **sole** source of presence
  truth. A friend is "online" **iff** present in this set.

**The two stores are independent** — `online` is *not* a subset view of
`friends` and neither cross-populates the other: presence is never inferred from
the buddy cache, and (the invariant below) the buddy cache / IM traffic is never
a presence signal. Independence is about *presence inference only* — it does
**not** mean the buddy cache is static. `friends` is kept **live** (next
subsection): a friendship formed mid-session is added when it forms.

**Live friendship additions & removals (the 2026-06-27 revision).** The buddy
cache must reflect a friendship the moment it forms — **never** wait for next
login's `FriendList`. Grounded in OpenSim's accept flow
(`FriendsModule.AddFriendship` / `StoreFriendships`), the two directions:

- **They accepted *our* offer.** We (the original offerer) receive a
  `FriendshipAccepted` IM (`ImDialog::FriendshipAccepted`, surfaced as
  `Event::InstantMessageReceived`) whose **`from_agent_id` is the new friend**.
  The inbound IM handler, on that dialog, inserts the friend into `friends`. No
  API change — the id is on the wire.
- **We accepted *their* offer.** The local `accept_friendship(transaction_id,
  calling_card_folder, now)` call carries **no** friend id (only the offer's
  `transaction_id`), and the accepter receives **no** `FriendshipAccepted` IM
  (OpenSim sends it only to the offerer) — just an `OnlineNotification`, not
  a "new friend" signal (it cannot be distinguished from an existing friend
  coming online, and presence must not feed the cache). So **`accept_friendship`
  gains a `friend_id: FriendKey` parameter** (and `Command::AcceptFriendship`
  gains the same field), and on accept the session inserts the friend. This is
  the **command-boundary** idiom the PERMISSION roadmap set (its `experience_id`
  on `AnswerScriptPermissions`): pass the datum the driver already holds — the
  offerer's id from the `FriendshipOffered` IM it is answering — through the
  command rather than tracking pending offers in the session.
- **Default rights on a fresh friendship.** OpenSim `StoreFriendships` writes
  `FriendRights.CanSeeOnline` for **both** directions and pushes **no**
  `ChangeUserRights` afterwards (verified — clients learn initial rights only
  from this default or the next buddy list). So a live-added `Friend` seeds
  `rights_granted = rights_received = FriendRights::CAN_SEE_ONLINE`; any later
  `ChangeUserRights` corrects a divergence. (SL's default matches —
  see-online is the standard new-friendship grant.)
- **Removal stays symmetric** — `FriendshipTerminated` (and our own
  `terminate_friendship`) drop the friend from **both** stores. With live
  add *and* live remove, `friends` tracks the true buddy list for the whole
  session, not just a login snapshot.

`from_agent_id` is an `AgentKey`; the cache keys on `FriendKey` — both wrap the
same `Key`/`Uuid`, so the insert converts via that shared id.

**Seeding & updates** (each hooks an *existing* handler, recording alongside
the event it already emits — the inbound event surface is unchanged):

| Signal | Site | Effect |
|--------|------|--------|
| `FriendList` (login buddy list) | build site `methods.rs:1078` | `friends` ← the `Vec<Friend>` (same `friend()`-mapped data the event carries); `online` starts **empty** |
| `FriendshipAccepted` IM (they accepted our offer) | IM dispatch (`ImDialog::FriendshipAccepted`) | insert `from_agent_id` into `friends`, default `CAN_SEE_ONLINE` both ways |
| `accept_friendship(friend_id, …)` (we accepted their offer) | the method (new `friend_id` arg) | insert `friend_id` into `friends`, default `CAN_SEE_ONLINE` both ways |
| `OnlineNotification` | `methods.rs:3504` | insert each `FriendKey` into `online` |
| `OfflineNotification` | `methods.rs:3514` | remove each `FriendKey` from `online` |
| `ChangeUserRights` | `methods.rs:3524` | mutate the cached `Friend`'s rights (see below) |
| `TerminateFriendship` | `methods.rs:2586` | remove `other` from **both** `friends` and `online` |

- **`online` starts empty at login** — the buddy list carries *rights*, not
  online status; presence arrives only as `OnlineNotification`s pushed after
  login (the passive model). So `friends` is full and `online` is empty,
  filling as notifications land.
- **`ChangeUserRights` →** `Event::FriendRightsChanged { friend_id, rights,
  granted_to_us }`. Map by direction onto the cached `Friend`: `granted_to_us ==
  true` updates `rights_received` (the rights the *friend* grants us);
  `granted_to_us == false` updates `rights_granted` (the echo of our own
  `grant_user_rights`). If `friend_id` is **absent** from `friends` (a rare race
  — a rights change racing ahead of the friendship-add signal), **ignore** it
  rather than synthesise a half-known entry; the friendship-add path seeds the
  full `Friend`, and a real rights change always follows an existing friendship.
- **`TerminateFriendship` →** `Event::FriendshipTerminated { other }` whose own
  doc says a buddy mirror "should drop `other`"; drop it from both stores so
  a former friend can never linger as online or in the roster.

**The presence invariant (the bug this design avoids).** `online` is mutated in
**only two** handlers — `OnlineNotification` (insert) and `OfflineNotification`
(remove) — plus `TerminateFriendship` removal. **No IM / chat-session handler
ever touches `online`.** This guards against the reference-viewer /
SL-grid bug where an IM just after a peer goes offline re-marks them online:
the A2 chat-session folding (`chat_session_mut`, message/typing/roster updates)
and presence are fully decoupled — IM traffic is **never** a presence signal.
`last_activity` (A2) is the *only* IM-driven timestamp and it lives on the
`ChatSession`, not on presence.

**Interaction with A7 (presence-driven auto-reset).** A3 maintains the presence
*state*; **A7** consumes it: when `OfflineNotification` removes a friend from
`online`, A7 (at the same handler) also clears that friend's typing, closes the
1:1 `ChatSession` whose peer is that friend, and best-effort drops them from
conference/group rosters. The two layer — A7 covers only *friend* participants
(friends-only presence); non-friend participants still rely on the sim's
`SessionLeave`. A3 only owns the `online` set transition; A7 owns the chat
fan-out.

**Persistence & reset.** Like `chat_sessions`, both are **grid-level** and
are **not** cleared at the `SitState` / teleport reset sites — presence does not
change because the agent teleported (A9). They clear only on logout (a `Closed`
session is dead; a relogin rebuilds them through the constructor and the fresh
`FriendList` seed), so no `close` hook is added — the A2/A9 convention.

**Accessors** (public, returning public types; the maps stay private):

    fn friends(&self) -> impl Iterator<Item = Friend> + '_   // the buddy cache
    fn friend(&self, id: FriendKey) -> Option<Friend>        // single lookup
    fn is_online(&self, friend: FriendKey) -> bool           // membership in `online`
    fn online_friends(&self) -> impl Iterator<Item = FriendKey> + '_

`is_online` semantics: **"known-online via an authoritative notification."**
Absence is *not* provable offline — a friend who does not grant us
`CAN_SEE_ONLINE` never generates a notification, so they are permanently absent
from `online` regardless of their real status. Callers must read absence as
"offline or not visible," never "definitely offline." The final accessor names /
shapes are confirmed in A10; A3 fixes the four listed in the task.
