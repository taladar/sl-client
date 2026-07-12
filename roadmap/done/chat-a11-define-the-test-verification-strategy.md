---
id: chat-a11
title: Define the test & verification strategy
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A11. Define the test & verification strategy.** Plan the
`sl-proto/tests/lifecycle.rs` / `sim_session.rs` cases: an inbound IM (each
kind) → the session opens, history records, unread increments; typing → the
typing set; `SessionAdd` / `SessionLeave` → the roster; `ConferenceInvited` →
a pending invite, accept → joined; `FriendList` + `FriendsOnline` /
`FriendsOffline` → the presence set; **`FriendsOffline` → typing cleared, the
1:1 session closed, and the friend dropped from a conference roster**; **a
teleport → sessions / history / presence preserved** (the inverse of the
`teleport_clears_seat` test); logout → cleared. List the remaining open
questions for sign-off (`ChatSessionRequest` vs UDP; the history retention
cap; the 1:1 key, peer vs `XOR` id; presence vs `SessionLeave` precedence;
**and the voice-channel cases of A12**).
**Done — see § Test & verification reference (from A11) + task B10 in
§ Phase B.** Decided: the strategy is **extend, don't duplicate** — every
new chat/presence assertion rides an **existing event-surfacing test** (the
events already fire; B-tasks add the *stored state*), reusing the harness
helpers `established(now)` (`:310`), `inbound_im(dialog, from_name,
message)` (`:1209`), `server_message` (`:288`), `drain` / `drain_events`
(`:248` / `:257`), and the existing IM / friend / group / conference /
invite / offline cases (`improved_instant_message_surfaces_event` `:1235`,
`online`/`offline_notification_surfaces_event` `:2398` / `:2429`,
`login_buddy_list_emits_friend_list` `:2355`,
`inbound_conference_send_surfaces_event` `:15007`,
`chatterbox_invitation_surfaces_conference_invited` `:15086`,
`read_offline_msgs_caps_surfaces_offline_ims` `:15048`). The **persistence**
case is the literal **inverse** of `teleport_clears_seat` (`:1716`): seed the
chat/presence stores, drive the same four region-boundary sites, assert they
**survive** (B10). Bidirectional round-trips go in `sim_session.rs` via its
`deliver_caps` (`:192`) / `setup` (`:208`) helpers, mirroring
`friendship_and_calling_cards_reach_client` (`:2395`) — a `SimSession` sends
the inbound IM / notification / `ChatterBoxInvitation` and the client store
reflects it. Runtime **parity** (B7) is tested in the crate command-dispatch
tests, plus the key **`Arc`-share / no-deep-copy** assertion (clone the reply
`Arc<[…]>`, compare pointers) and a **bevy-direct vs tokio-query data-parity**
check. **All A11 open questions are RESOLVED for the A1–A11 core** (each by
its deciding item, listed in the reference): `ChatSessionRequest` vs UDP →
**both** (A5; OpenSim stubs the cap so UDP is the local-grid path, CAPS is
aditi-only); history cap → **256 pop-front** (A8); the 1:1 key → **peer
`AgentKey`**, `XOR` derivable both ways (A2); presence vs `SessionLeave` →
they **layer**, both idempotent (A7); plus the A10-surfaced exposure model →
**bevy-borrow / tokio-pull** (A10, user-decided). The **only** still-open
cases are **A12 (voice)** and **A13 (file logs)**, whose tests append to B10
when those items are designed — so A11 signs off the **A1–A11 core**,
and Phase A formally completes once A12 + A13 land.

## Test & verification reference (from A11)

The verification plan for the whole chat-session + presence system. The guiding
rule is **extend, don't duplicate**: nearly every behaviour already has an
*event-surfacing* test in `sl-proto/tests/lifecycle.rs` (the events fire as
a stateless pass-through); the B-tasks add **stored state**, so each new test is
the existing case **plus** an assertion on the new accessor. No new harness is
needed — the existing helpers cover it.

**Harness in hand** (`lifecycle.rs`): `established(now)` (`:310`) builds a
logged-in session; `inbound_im(dialog, from_name, message)` (`:1209`) forges an
`ImprovedInstantMessage`; `server_message` (`:288`) frames any inbound message;
`drain` (`:248`) / `drain_events` (`:257`) pump outbound datagrams / events;
`teleport_clears_seat` (`:1716`) is the persistence inverse template. For the
bidirectional path, `sim_session.rs` has `setup` (`:208`), `pump` (`:151`),
`drain_server` (`:170`), and `deliver_caps` (`:192`, drives the real
EventQueueGet → `handle_caps_event` path). The bidirectional template is
`friendship_and_calling_cards_reach_client` (`:2395`) — a `SimSession` sends,
the client store reflects.

**Per-task case map** (each row = the existing event test it extends + the new
state assertion):

| B-task | Extends / new test | New assertion |
|--------|--------------------|---------------|
| B2 registry | unit | insert/lookup per `ChatSessionKind`; `chat_session_mut` creates once + restamps `last_activity`; reverse-XOR round-trip incl. self-IM |
| B1 presence | `login_buddy_list_emits_friend_list` (`:2355`), `online`/`offline_notification_surfaces_event` (`:2398`/`:2429`), `change_user_rights_from_friend_surfaces_event` (`:2453`), `terminate_friendship_surfaces_former_friend` (`:7319`) | `friends()` seeded + `online` empty at login; online/offline insert/remove; rights by direction; unknown-friend rights ignored; terminate drops both; `FriendshipAccepted` IM + `accept_friendship(friend_id)` both add live; **IM-after-offline invariant** (deliver an IM after `OfflineNotification` → peer stays offline) |
| B5 lifecycle | `group_session_message_surfaces_event` (`:3099`), `inbound_conference_send_surfaces_event` (`:15007`), `improved_instant_message_surfaces_event` (`:1235`) | outbound `start_*` opens `Joined`; inbound group/conf message opens `Joined` + promotes a seeded `Invited`; 1:1 opens `Joined`; `leave_*` removes; a 1:1 is never removed by a leave |
| B5 invites | `chatterbox_invitation_surfaces_conference_invited` (`:15086`) | text invite → `Invited(channel=Text)` (group + conf); voice → `Voice`/`Both`; `accept_chat_invite` → `Joined`, inbound traffic also promotes; `decline_chat_invite` removes; per-channel CAPS `method`/`session-id` LLSD unit; accept-reply roster decodes to participants |
| B3 roster/typing | `improved_instant_message_typing_surfaces_im_typing` (`:1263`), the participant arms | `SessionAdd`/`SessionLeave` insert/remove + open session; `participants(Direct)` = `{peer}`; `ImTyping` start/stop sets/clears; 1:1 typing keys by `from_agent_id`; typing does **not** open a session; entry expires after `poll` passes `TYPING_TIMEOUT`; `TypingStop` clears now |
| B6 presence-reset | new (seed roster + 1:1 typing for a friend) | `OfflineNotification` → friend gone from roster + `typing`, **but** sessions still exist + `is_online` false; a non-friend is untouched (only `SessionLeave` removes them); `FriendsOnline` re-adds, changes no session |
| B4 history/unread | `improved_instant_message_surfaces_event` (`:1235`), `read_offline_msgs_caps_surfaces_offline_ims` (`:15048`) | inbound 1:1 logs `unread==1`; group `SessionSend` logs to the group session; own outbound logs (`sender=self`) + resets `unread`; `mark_session_read` resets; `HISTORY_CAP + 1` drops oldest; an `offline==true` IM logs its wire `timestamp` + bumps `unread`; `total_unread` sums |
| B10 persistence | **inverse of** `teleport_clears_seat` (`:1716`) | seed session + history + roster + presence, drive `begin_handover` / `promote_child_to_root` / `TeleportLocal` / child `DisableSimulator` → **all unchanged**; `LogoutReply` keeps them readable on the closed session + a fresh `Session::new` is empty |
| B7 exposure | crate command-dispatch tests | `QueryChatSessions` → light list newest-first, `Invited` flattened; `QueryChatHistoryPage` → bounded newest-first page + `prev`, paging walks older windows without materialising the whole history; `QueryFriends` → right `online` flag; replies are `Arc<[…]>` (clone the `Arc`, compare pointers = no deep copy); a bevy-direct read of `chat_sessions_info()` matches the tokio query reply (data parity) |

**Bidirectional round-trips (`sim_session.rs`).** A handful of end-to-end cases
confirm the inbound decode + fold under a real `SimSession`: the sim sends an
`ImprovedInstantMessage` (each kind), an `Online`/`OfflineNotification`,
and a `ChatterBoxInvitation` (via `deliver_caps`), and the client's
`chat_sessions` / `friends` / `online` reflect them — the inbound mirror of the
existing `friendship_and_calling_cards_reach_client`. These guard the wire
decode, not just the in-memory fold (which `lifecycle.rs` covers directly).

**Runtime parity (B7).** The three runtimes are checked with the established
command-dispatch tests plus the two distinctive A10 assertions: (1) the reply
payloads are genuinely `Arc`-shared (no deep copy on hand-off), and (2) a
bevy-style direct `&Session` read returns the **same data** as the tokio
query/reply — so "parity" (same data + Commands + views, transport differing) is
verified, not assumed.

**Open questions — resolution for sign-off.** Every question A11 was meant to
surface is now **answered by an earlier item**; the table records the decision
and where it lives, so Phase A's core has no dangling design choice:

| Open question | Resolution | Item |
|---------------|-----------|------|
| `ChatSessionRequest` (modern CAPS) vs UDP implicit-join | **both** — CAPS where the cap exists, UDP fallback; OpenSim *stubs* the cap, so UDP is the local-grid path and CAPS is aditi-only-testable | A5 |
| History retention cap | bounded `VecDeque`, `HISTORY_CAP = 256`, pop-front oldest; long-term scrollback is the A13 file layer | A8 |
| The 1:1 key — peer vs `XOR` id | **peer `AgentKey`**; the `XOR` `ImSessionId` is derivable both ways (`compute_im_session_id` / `direct_peer_from_session_id`) so wire-only signals map back | A2 |
| Presence vs `SessionLeave` precedence | they **layer** (both idempotent): A7 is the fast friends-only path, `SessionLeave` the universal one; neither replaces the other | A7 |
| Read-model exposure across runtimes (surfaced by A10) | **bevy direct `&Session` borrow / tokio + REPL pull** (`Arc`-shared, paged); parity = same data + Commands + views | A10 |
| **Voice-channel cases** | **STILL OPEN** — A12 not yet designed; its tests append to B10 | A12 |
| **Local chat-log file cases** | **STILL OPEN** — A13 not yet designed; its tests append to B10 | A13 |

So **A11 signs off the A1–A11 core** test plan with no open core questions; the
remaining two rows are the still-undesigned **A12 (voice)** and **A13 (file
logs)**. Phase A completes — and Phase B may begin — only once those two
land and their cases are folded into B10.
