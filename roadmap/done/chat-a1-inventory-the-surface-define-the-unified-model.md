---
id: chat-a1
title: Inventory the surface & define the unified model
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A1. Inventory the surface & define the unified model.** Enumerate the
three chat kinds and their id derivation (1:1 `XOR`, conference caller-minted
`ImSessionId`, group = group id); the command / method / event / `ImDialog`
surface; the two delivery paths (UDP `ImprovedInstantMessage` vs CAPS
`ChatterBoxInvitation`); and the friend-presence surface (`FriendList`,
`FriendsOnline` / `FriendsOffline`, `FriendRightsChanged`; friends-only,
`CAN_SEE_ONLINE`-gated, passive). Define a unified `ChatSession` concept
(`Direct { peer } | Group { group_id } | Conference { id }`) and a
presence/buddy concept, and state the boundary (local chat is OUT).
**Done — see § Inventory & unified-model reference (from A1) + task B2 in
§ Phase B.** Decided the unified discriminator
`ChatSessionKind { Direct { peer: AgentKey } | Group { group_id: GroupKey } |
Conference { id: ImSessionId } }` (typed ids, never raw `Uuid`) with a
canonical-id derivation per kind, and confirmed the buddy/presence concept
reuses the existing `Friend` struct + `FriendKey`. **Correction to § Protocol
reality:** offline-IM *retrieval* is **already implemented** — both
`Command::RetrieveInstantMessages` (legacy UDP) and
`Command::RequestOfflineMessages` (modern `ReadOfflineMsgs` CAPS) shipped with
the `MISSING_ROADMAP.md` outbound work — so A8 plans only the *bounded log /
unread* model, not the fetch path (the § Protocol reality bullet is updated).

## Inventory & unified-model reference (from A1)

The complete inbound/outbound/event surface of the IM-session + friend-presence
system as it exists **today** (a stateless pass-through), and the unified model
the chat-session state will be built around. Every type/method/event below is
real and already in the tree — A1 adds **no** code, only the inventory and the
model. The simulator stays authoritative throughout; the planned `Session` state
is an API-convenience mirror, never a security or routing boundary.

**The three chat kinds, one wire message.** All IM traffic rides
`ImprovedInstantMessage` (`message_template.msg`, `Low 254`); the `ImDialog`
byte (`types/chat.rs:278`, 27 variants incl. `Unknown(u8)`) carries the
semantics and `InstantMessage::from_group` (`types/chat.rs:421`) splits group
from conference on the session dialogs. The session dialogs:
`SessionGroupStart` (15) / `SessionConferenceStart` (16) open a session;
`SessionSend` (17) is a message in one; `SessionAdd` (13) /
`SessionOfflineAdd` (14) / `SessionLeave` (18) move participants;
`TypingStart` (41) / `TypingStop` (42) are typing. Ordinary 1:1 is
`Message` (0). The inbound demux lives in the `ImprovedInstantMessage` arm of
`dispatch` (`session/methods.rs:1991`), branching on `from_group`
(`:2005` group, `:2025` conference).

**Id derivation per kind** (the unified-model keys):

| Kind | Canonical id | Source |
|------|-------------|--------|
| 1:1 **Direct** | `XOR(agent_id, peer)` | `compute_im_session_id(agent, other)` (`session/conversions.rs:808`) — deterministic, symmetric |
| **Group** | the `group_id` itself | `GroupKey`; the session id *is* the group id |
| **Conference** | a caller-minted `ImSessionId` | `ImSessionId` (`bookkeeping_ids.rs:205`), fresh per conference |

**Outbound surface** — `Command`s (`command.rs`) → `Session` methods
(`session/methods.rs`) → circuit `send_*` (`session/circuit.rs`):

| Command | Method | Kind |
|---------|--------|------|
| `InstantMessage { to_agent_id, message }` | `send_instant_message` (`:3809`) | Direct |
| `ImTyping { to_agent_id, typing }` | `send_im_typing` (`:3835`) | Direct typing |
| `StartGroupSession { group_id }` | `start_group_session` (`:4727`) | Group |
| `SendGroupMessage { group_id, message }` | `send_group_message` (`:4748`) | Group |
| `LeaveGroupSession { group_id }` | `leave_group_session` (`:4768`) | Group |
| `StartConference { session_id: ImSessionId, invitees, message }` | `start_conference` (`:5447`) | Conference |
| `SendConferenceMessage { session_id, message }` | `send_conference_message` (`:5485`) | Conference |
| `LeaveConference { session_id }` | `leave_conference` (`:5515`) | Conference |
| `RetrieveInstantMessages` | `retrieve_instant_messages` (`:5543`) | Offline (UDP) |
| `RequestOfflineMessages` | (CAPS GET `ReadOfflineMsgs`) | Offline (CAPS) |

**Inbound event surface** (`types/event.rs`), emitted by the demux with **no**
state retained:

| Event | Carries | Kind |
|-------|---------|------|
| `InstantMessageReceived(Box<InstantMessage>)` (`:389`) | full IM (all non-session dialogs incl. offline) | Direct / system |
| `ImTyping { from_agent_id, from_agent_name, session_id, typing }` (`:393`) | typing | Direct typing |
| `GroupSessionMessage { group_id, from_agent_id, from_name, message }` (`:753`) | message | Group |
| `GroupSessionParticipant { group_id, agent_id, joined }` (`:766`) | add/leave | Group roster |
| `ConferenceSessionMessage { session_id, from_agent_id, from_name, message }` (`:778`) | message | Conference |
| `ConferenceSessionParticipant { session_id, agent_id, joined }` (`:791`) | add/leave | Conference roster |
| `ConferenceInvited { session_id, from_agent_id, from_name, dialog, from_group, session_name, message, region_id, position, parent_estate_id, timestamp, binary_bucket }` (`:803`) | invitation | Conference / group invite |

Note the existing session events carry a **raw `Uuid` `session_id`** (e.g.
`ImTyping`, `ConferenceSessionMessage`, `ConferenceInvited`), not the typed
`ImSessionId` / `GroupKey`. That is pre-existing wire-adjacent typing; the
unified model below keys on the **typed** ids, and whether to retrofit those
event fields to typed ids is an optional cleanup A10 may schedule (not required
for the state system).

**The two delivery paths.** (1) **UDP** `ImprovedInstantMessage` — every dialog
above, demuxed at `session/methods.rs:1991`. (2) **CAPS** — modern conference
invitations arrive as `ChatterBoxInvitation`
(`chatterbox_invitation_from_llsd`, `session/conversions.rs:2511`; handled in
`handle_caps_event` at `:663` → `Event::ConferenceInvited`), and offline IMs as
`ReadOfflineMsgs` (`offline_messages_from_llsd`, `:2440`; `handle_caps_event`
at `:652`). **`ChatSessionRequest`** (the modern accept/decline + session-ops
capability) is **confirmed not implemented** — no reference anywhere in the
tree; today you "accept" an invite only by implicitly sending into the session
over UDP. A5 decides whether to adopt it.

**Friend-presence surface** (the folded-in concern):

- **Buddy list** — `Event::FriendList(Vec<Friend>)` emitted once at login
  (`session/methods.rs:1079`) from the login response's `buddy_list`
  (`friend(entry)` at `session/conversions.rs:961`). `Friend`
  (`types/avatar_profile.rs:317`) = `{ id: FriendKey, rights_granted:
  FriendRights, rights_received: FriendRights }`. **`FriendKey`**
  (`sl-types key.rs:216`) is the friend identity newtype.
- **Presence** — `Event::FriendsOnline(Vec<FriendKey>)` (`event.rs:524`, from
  `OnlineNotification`, `methods.rs:3504`) and
  `Event::FriendsOffline(Vec<FriendKey>)` (`event.rs:526`, from
  `OfflineNotification`, `:3514`). **Passive** — confirmed no
  `RequestOnlineNotification` outbound exists. **Friends-only,
  `CAN_SEE_ONLINE`-gated** (`FriendRights`, `sl-types friend.rs:12`:
  `CAN_SEE_ONLINE` `1<<0`, `CAN_SEE_ON_MAP` `1<<1`, `CAN_MODIFY_OBJECTS`
  `1<<2`).
- **Rights changes** — `Event::FriendRightsChanged { friend_id, rights,
  granted_to_us }` (`event.rs:531`, from `ChangeUserRights`, `:3524`);
  outbound `Command::GrantUserRights { target, rights }` (`command.rs:339`).
  Friendship lifecycle (offer/accept/decline/terminate) and calling cards also
  exist as commands/events but are **out of the chat-session core** (A3 may
  reference them for the roster, not own them).
- **No friend/presence state is stored today** — confirmed the `Session` struct
  (`session.rs:890`) holds *no* `friends` / `online` field; presence is a pure
  event pass-through.

**Where new state will live.** The `Session` struct (`session.rs:890`) already
holds `sit: SitState` (`:931`), `teleport: TeleportPhase` (`:935`),
`objects` (`:1004`), `own_avatar: BTreeMap<CircuitId, RegionLocalObjectId>`
(`:1034`), and the `events` queue (`:1051`). The chat-session registry and the
buddy/presence cache will sit **beside** `sit` / `teleport` as private fields
reached only through accessors — the exact `7bc19b4` precedent. Because chat /
presence are **grid-level**, they will be the *inverse* of `SitState`: they
**persist** across the teleport/crossing reset sites (A9), clearing only on
logout.

**The unified model (the A1 deliverable).** A single discriminator names which
of the three kinds a session is, carrying the kind's *typed* id (never a raw
`Uuid`):

    enum ChatSessionKind {
        Direct { peer: AgentKey },        // 1:1; canonical id = XOR(agent, peer)
        Group { group_id: GroupKey },     // canonical id = group_id
        Conference { id: ImSessionId },   // caller-minted conference id
    }

- **Direct** is keyed by the *peer* `AgentKey` (human-meaningful, stable); the
  `XOR` `ImSessionId` is *derivable* on demand via `compute_im_session_id` for
  wire correlation. Whether the registry key is the peer or the `XOR` id is
  **A2's** decision — A1 only fixes that both are available and equivalent.
- **Group** is keyed by `GroupKey` (≡ the session id on the wire).
- **Conference** is keyed by the minted `ImSessionId`.
- A `ChatSession` value (designed in A2) wraps a `ChatSessionKind` plus the
  per-session state the later items add (participants A6, typing A6,
  history/unread A8, invite status A5, voice state A12, `last_activity`).
  A session carries **both a text and a voice channel** (the 2026-06-27 scope
  expansion); both live under the one `ChatSessionKind` / session id, not as
  separate sessions.
- The **buddy/presence concept** reuses the **existing** `Friend` struct +
  `FriendKey` — no new identity type. A3 designs the cache (a `Friend` map +
  an online `BTreeSet<FriendKey>`).

**Boundary (explicit).** **IN scope:** the three IM-session kinds (Direct /
Group / Conference), their rosters / typing / history / unread / invitations,
the per-session **voice channel at the SL-signalling level** (has-voice, channel
info, join/leave-voice, voice membership — A5 / A12; **scope expanded
2026-06-27**), folded-in **friend presence** (buddy cache + online set +
presence-driven auto-reset), and the optional **local chat-log files** (A13 —
runtime read/write, Firestorm-style, covering **all** text-chat types **incl.
nearby**). **OUT of scope:** local proximity chat's **session state**
(`ChatFromViewer` / `say` → `ChatReceived`) — a separate stateless concern
(though nearby chat **is** logged by A13); the **Vivox / WebRTC audio transport
and the "who is speaking"
indicators** (the external voice client — sl-client does voice *signalling*
only); the full friendship lifecycle and calling-card flows (referenced for
rosters/presence, but their commands/events are unchanged); and offline-IM
**retrieval** (already shipped — see the § Protocol reality correction; only the
*log/unread* model is planned, A8). The system is mostly a **read model** (it
mirrors the wire and exposes accessors); its only outbound actions are the
existing commands plus the A5 accept/decline.
