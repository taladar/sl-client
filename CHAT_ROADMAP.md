# chat session road map

A plan to give the SL client a *stateful* chat-session system covering the three
instant-message session kinds — 1:1 direct IM, ad-hoc conferences, and group
chat — with **friend presence** folded in. Today this whole surface is a
stateless pass-through: inbound `ImprovedInstantMessage` is decoded and fanned
out to events (`InstantMessageReceived`, `ImTyping`,
`Group`/`ConferenceSessionMessage`, `…Participant`, `ConferenceInvited`), the
buddy list arrives once as `Event::FriendList`, and `OnlineNotification` /
`OfflineNotification` arrive as `Event::FriendsOnline` / `FriendsOffline` — but
**no `Session` state** tracks open sessions, rosters, typing, history, pending
invitations, or who is online. This roadmap plans a system that keeps that
state for the library user and resets the chat state tied to a friend when that
friend goes offline. Work these top-to-bottom; tick a box only when the step
builds, is clippy-clean (restriction lints), and `cargo test` passes. Add
sub-tasks as you discover them.

Phase A is **planning only** — its items produce design decisions, not code.
Phases B+ (implementation) are defined once Phase A is signed off.

Scope reminders:

- Commit on the current branch only (never auto-create a feature branch).
- `Session` (sl-proto) is sans-IO: the chat/presence state lives there, beside
  `TeleportPhase` / `SitState`, driven by inbound messages and the outbound
  commands.
- Keep `sl-client-tokio` and `sl-client-bevy` (and the REPL) at feature parity.
- Never push client-only protocol types into the shared `sl-types` crate.
- Local proximity chat (`ChatFromViewer` / `say` → `Event::ChatReceived`) is a
  **separate** concern and **out of scope** here; this roadmap is about IM /
  conference / group **sessions**.
- Wrap this file at 80 columns; fmt/clippy/rumdl green before commit (the ggh
  hook rejects MD013 and re-runs clippy).

## Protocol reality (constraints Phase A must respect)

- One wire message carries all three chat kinds: `ImprovedInstantMessage`
  (`message_template.msg`, `Low 254`); the `ImDialog` byte (`types/chat.rs`)
  distinguishes the semantics and the `from_group` flag separates group
  (`true`) from conference (`false`) on `SessionSend` / `SessionAdd` /
  `SessionLeave`.
- Session-id semantics differ per kind: **1:1** = the deterministic
  `XOR(agent_id, peer)` (`compute_im_session_id` in `session/conversions.rs`);
  **conference** = a caller-minted `ImSessionId`; **group** = the group id
  itself.
- Modern invitations arrive over CAPS as `ChatterBoxInvitation` →
  `Event::ConferenceInvited`. The modern `ChatSessionRequest` capability
  (accept/decline and other session operations) is **not** implemented — only
  the UDP `ImprovedInstantMessage` path is. There is no accept/decline today;
  you join a session implicitly by sending into it.
- Inbound offline IMs already surface (`offline = true`), and **offline-IM
  history retrieval is now implemented** (A1 correction): the modern
  `ReadOfflineMsgs` CAPS (`Command::RequestOfflineMessages`,
  `offline_messages_from_llsd` in `session/conversions.rs`) *and* the legacy
  `RetrieveInstantMessages` UDP (`Command::RetrieveInstantMessages`,
  `send_retrieve_instant_messages` in `session/circuit.rs`) both ship — both
  re-deliver as offline `Event::InstantMessageReceived`. They were added by the
  `MISSING_ROADMAP.md` outbound work *after* this roadmap was drafted, so A8
  plans only the bounded per-session **log / unread** model, not the fetch path.
- Friend presence is **friends-only**, `CAN_SEE_ONLINE`-gated and bidirectional
  (confirmed in OpenSim `FriendsModule.cs`), and **passive** — the simulator
  pushes `OnlineNotification` / `OfflineNotification`; there is no
  `RequestOnlineNotification`. The rights flags are
  `sl_types::friend::FriendRights`: `CAN_SEE_ONLINE`, `CAN_SEE_ON_MAP`,
  `CAN_MODIFY_OBJECTS`.
- Chat sessions, history, and presence are **grid-level** (routed by the grid's
  IM / group / presence services, not the region simulator), so unlike
  `SitState` and script permissions they **persist** across teleport and region
  crossings — the *inverse* of those resets.
- No chat or presence state exists in the `Session` struct (`session.rs`); it
  would live beside the `TeleportPhase` / `SitState` enums (the precedent from
  commit `7bc19b4`).

## Phase A — plan the chat-session + presence system (design only; no code yet)

- [x] **A1. Inventory the surface & define the unified model.** Enumerate the
  three chat kinds and their id derivation (1:1 `XOR`, conference caller-minted
  `ImSessionId`, group = group id); the command / method / event / `ImDialog`
  surface; the two delivery paths (UDP `ImprovedInstantMessage` vs CAPS
  `ChatterBoxInvitation`); and the friend-presence surface (`FriendList`,
  `FriendsOnline` / `FriendsOffline`, `FriendRightsChanged`; friends-only,
  `CAN_SEE_ONLINE`-gated, passive). Define a unified `ChatSession` concept
  (`Direct { peer } | Group { group_id } | Conference { id }`) and a
  presence/buddy concept, and state the boundary (local chat is OUT).
  **Done — see § Inventory & unified-model reference (from A1) + task B1 in
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
- [x] **A2. Design the chat-session state model & keying.** Specify what
  `Session` stores (beside `TeleportPhase` / `SitState`): a registry keyed by
  the canonical session id → `ChatSession { kind, participants, typing,
  last_activity, unread / last_read, invite status }`. Decide how 1:1 sessions
  are lazily opened (on the first inbound/outbound IM under the `XOR` id), the
  participant source (`SessionAdd` / `SessionLeave`), and whether the 1:1 key
  stores the peer `AgentKey` or the `XOR` `ImSessionId`.
  **Done — see § State-model & keying reference (from A2) + task B2 in
  § Phase B.** Decided: one private field
  `chat_sessions: BTreeMap<ChatSessionKind, ChatSession>` on `Session`, the A1
  `ChatSessionKind` (carrying the typed id per kind) **doubling as key** — so
  the `kind` is the *key*, not a value field (resolves the sketch's redundant
  `kind`), and the three id-spaces are disjoint by construction (no flat-`Uuid`
  collision worry). **1:1 keyed by peer `AgentKey`** (`Direct { peer }`), not
  the `XOR` id: the peer is what the typed IM surface already carries, and the
  `XOR` `ImSessionId` is *derivable both ways* (XOR is self-inverse) so a
  wire-only 1:1 signal keyed by the `XOR` id (`ImTyping`) maps back to the peer.
  `ChatSession` (value) holds only mutable state — `participants` /
  `typing: BTreeSet<AgentKey>` (A6), `last_activity: Instant` (the only
  field A2 fills), with history/unread (A8) and invite status (A5) added
  additively. Lazy get-or-create via `chat_session_mut(kind, now)`; *which*
  event opens *which* kind is A4's lifecycle call.
- [x] **A3. Design the friend-presence state model.** A buddy-list cache
  (`Friend { id, rights_granted, rights_received }`) and an online set keyed by
  `FriendKey`, seeded by `FriendList` at login and updated by `FriendsOnline` /
  `FriendsOffline` and `FriendRightsChanged`. Presence is friends-only /
  `CAN_SEE_ONLINE`-gated / passive. Drive the online set **only** from the
  authoritative presence notifications (and the login buddy list) — never infer
  presence from IM send/receive activity. (Known reference-viewer / SL-grid bug
  to **avoid replicating**: an IM sent immediately after a peer goes offline
  falsely re-marks them online; this design must ignore IM traffic as a presence
  signal.) Accessors: `friends()`, `is_online(friend)`, `online_friends()`.
  **Done — see § Friend-presence reference (from A3) + task B3 in § Phase B.**
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
- [x] **A4. Design the session lifecycle (open / join / send / leave / close).**
  1:1 implicit on the first message; group via `start_group_session` (decide
  whether an inbound group message also opens/tracks it); conference via
  `start_conference` (caller mints the id) or via accepting an invite. Define
  what marks a session *active/joined* versus *pending* (there is no UDP
  "joined" ack) and what removes it from the registry (an explicit leave,
  logout).
  **Done — see § Session-lifecycle reference (from A4) + task B4 in § Phase B.**
  Decided a `lifecycle: ChatSessionLifecycle { Invited | Joined }` field on
  `ChatSession` (this *is* the A2-deferred "invite status"): **1:1 is always
  `Joined`** the instant it opens (no handshake); **group / conference open
  `Joined` optimistically** on our `start_*`/accept *or* on **any inbound
  message/participant traffic** (yes — an inbound group/conference message opens
  & tracks the session, promoting an `Invited` entry to `Joined`); **`Invited`**
  is set *only* by a bare invitation with no traffic yet (A5 feeds it). There is
  **no UDP "joined" ack**, so `Joined` is *optimistic* ("we believe we
  are in"), not sim-confirmed. **Removal:** an explicit `leave_group_session` /
  `leave_conference` **removes** the entry; an A5 decline removes the `Invited`
  entry; **logout** clears all (constructor rebuild, no `close` hook — the
  A2/A9 convention). **1:1 is never removed** by a leave (no such op) — it
  persists to logout (A7 may *mark* it on peer-offline, never remove). No new
  command (the start/send/leave surface already exists; A5 adds accept/decline).
- [ ] **A5. Design invitation handling + accept/decline.** A pending-invitations
  registry fed by `Event::ConferenceInvited` (and group invites), plus new
  accept/decline commands. Decide the path: adopt the modern
  `ChatSessionRequest` capability (its accept-invitation method; not implemented
  today) versus the UDP implicit-join. Output: the invitation lifecycle and the
  new command(s).
- [ ] **A6. Design participant & typing tracking.** From
  `Group` / `ConferenceSessionParticipant` and `ImTyping`, maintain per-session
  rosters and a per-session typing set; define accessors
  (`participants(session)`, `typing(session)`). Decide how outbound
  `send_im_typing` interacts and whether typing entries auto-expire.
- [ ] **A7. Design presence-driven auto-reset.** On `FriendsOffline`, for each
  offlined friend: clear their typing in every session; mark/close the open
  **1:1** session whose peer is that friend; and best-effort update **conference
  / group rosters** where they appear as a participant (drop or mark-left).
  State the caveat explicitly: presence is friends-only, so this only covers
  friend-participants who grant see-online — **non-friend** participants still
  rely on the simulator's `SessionLeave` events. The two signals layer; they do
  not replace each other. On `FriendsOnline`: update the presence set (and
  optionally clear a stale "peer offline" marker on the 1:1 session); no other
  auto-action. Define the exact session transitions.
- [ ] **A8. Design message history, unread & offline retrieval.** Plan a bounded
  per-session message log (sender, timestamp, text, dialog), an unread /
  last-read marker per session, and offline-IM retrieval — the modern
  `ReadOfflineMsgs` CAPS (and/or the legacy `RetrieveInstantMessages` UDP),
  neither implemented yet. Decide retention bounds (cap the log length), what
  counts as unread, and how login drains queued offline IMs into the right
  sessions.
- [ ] **A9. Lock the persistence-vs-region behaviour.** Chat sessions, history,
  and presence are **grid-level** and **persist** across teleport
  (`begin_handover`, `TeleportLocal`), neighbour crossing
  (`promote_child_to_root`), and `DisableSimulator` — explicitly **not** reset
  (the inverse of the `SitState` reset at those same sites). All of it clears
  only on logout (`SessionState::Closed`). Decide whether any persistence beyond
  a single session is in scope (default: in-memory only).
- [ ] **A10. Specify the API-surface delta & driver/REPL exposure.** Enumerate
  the new/changed `Command`s (accept/decline invitation, an optional open/close
  session, request offline IMs), any `Event` changes, and the new `Session`
  accessors (`sessions()`, `session(id)`, participants, typing, history, unread,
  `friends()`, `is_online`); and how `sl-client-tokio`, `sl-client-bevy`, and
  the REPL expose them at feature parity. Draw the boundary between sl-proto
  `Session` state and application policy.
- [ ] **A11. Define the test & verification strategy.** Plan the
  `sl-proto/tests/lifecycle.rs` / `sim_session.rs` cases: an inbound IM (each
  kind) → the session opens, history records, unread increments; typing → the
  typing set; `SessionAdd` / `SessionLeave` → the roster; `ConferenceInvited` →
  a pending invite, accept → joined; `FriendList` + `FriendsOnline` /
  `FriendsOffline` → the presence set; **`FriendsOffline` → typing cleared, the
  1:1 session closed, and the friend dropped from a conference roster**; **a
  teleport → sessions / history / presence preserved** (the inverse of the
  `teleport_clears_seat` test); logout → cleared. List the remaining open
  questions for sign-off (`ChatSessionRequest` vs UDP; the history retention
  cap; the 1:1 key, peer vs `XOR` id; presence vs `SessionLeave` precedence).

Phase A scopes the planning only; the implementation tasks each Phase A item
produces are appended to **Phase B** below as that item is worked, tagged with
the producing item. Phase B is a *draft* until Phase A is signed off; tick a box
only when the step builds, is clippy-clean (restriction lints), and `cargo test`
passes. Keep `sl-client-tokio`, `sl-client-bevy`, and the REPL at feature
parity; never push client-only types into shared `sl-types`.

## Phase B — implementation (tasks produced by Phase A)

Each Phase A item appends here the concrete implementation task(s) it implies,
plus a **reference** subsection recording the design knowledge it produced. The
list is a first draft and will be consolidated once Phase A is complete (the
`PERMISSION_ROADMAP.md` precedent). Do **not** start a Phase B task until
Phase A is signed off.

### Inventory & unified-model reference (from A1)

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
  history/unread A8, invite status A5, `last_activity`).
- The **buddy/presence concept** reuses the **existing** `Friend` struct +
  `FriendKey` — no new identity type. A3 designs the cache (a `Friend` map +
  an online `BTreeSet<FriendKey>`).

**Boundary (explicit).** **IN scope:** the three IM-session kinds (Direct /
Group / Conference), their rosters / typing / history / unread / invitations,
and folded-in **friend presence** (buddy cache + online set + presence-driven
auto-reset). **OUT of scope:** local proximity chat (`ChatFromViewer` / `say` →
`Event::ChatReceived`) — a separate stateless concern; the full friendship
lifecycle and calling-card flows (referenced for rosters/presence, but their
commands/events are unchanged); and offline-IM **retrieval** (already shipped —
see the § Protocol reality correction; only the *log/unread* model is planned,
A8). The whole system is a **read model**: it mirrors what the wire reports and
exposes accessors; it issues no protocol on its own beyond the existing
commands.

### B1. Define the unified `ChatSessionKind` discriminator (from A1)

Introduce the foundational, *typed* session-kind discriminator the whole
registry keys off, with the canonical-id derivation, but **no** stored state yet
(that is B2/A2). Concretely:

- Add `ChatSessionKind { Direct { peer: AgentKey } | Group { group_id: GroupKey
  } | Conference { id: ImSessionId } }` in a new chat-session module under
  `sl-proto/src/types/` (or `session/`), with derives matching the crate
  convention (`Debug, Clone, Copy, PartialEq, Eq` + `Ord` if it will be a map
  key — A2 confirms). Doc each variant with its id semantics.
- Add a canonical-id helper that maps a kind to its wire-correlation
  `ImSessionId` (Direct → `compute_im_session_id`; Group → the group id reused
  as an `ImSessionId`; Conference → the id verbatim), reusing the existing
  `compute_im_session_id`.
- No `Session` field, no command, no event in B1 — it is the type skeleton A2's
  registry and A3's presence cache build on. Lands with unit tests for the
  derivation (the `XOR` symmetry round-trip; group/conference identity).

This task stays **drafted/blocked** until Phase A is signed off; A2 may fold it
into the registry task (B2) during the Phase B consolidation.

### State-model & keying reference (from A2)

How the chat-session registry is shaped, keyed, and lazily populated. The
simulator stays authoritative; this is an API-convenience **read model** (A1) —
it mirrors what the wire reports and never routes or gates traffic. A2 designs
*storage + keying* only; the per-event folding lives in later items (rosters /
typing A6, lifecycle A4, invitations A5, history / unread A8, presence-driven
mutation A7).

**The registry.** One new private field on `Session` (`session.rs`), beside
`sit: SitState` (`:931`) / `teleport: TeleportPhase` (`:935`), reached only
through accessors (the `7bc19b4` precedent):

    chat_sessions: BTreeMap<ChatSessionKind, ChatSession>

- **The key is the A1 `ChatSessionKind` itself** — it already carries the typed
  id per kind (`Direct { peer: AgentKey }` / `Group { group_id: GroupKey }` /
  `Conference { id: ImSessionId }`), so it *is* the canonical session id. This
  **resolves the A2 sketch's redundant `kind` field**: the kind/id lives in the
  key, and `ChatSession` (the value) stores only mutable per-session state.
- **No id-space collision** (unlike Firestorm, keying `mId2SessionMap` by the
  bare session `UUID`): the three ids are all `Uuid`-backed, but the enum
  discriminant keeps them disjoint, so a group id never aliases a conference id
  or a 1:1 `XOR` id in the map.
- **`BTreeMap`** (not `HashMap`) keeps the crate's deterministic-iteration
  convention, so `ChatSessionKind` **must derive `Ord`** (confirming B1's "+
  `Ord` if it will be a map key" — it will). All three payloads (`AgentKey` /
  `GroupKey` / `ImSessionId`) are `Copy + Ord`, so the enum is `Copy + Ord`.

**The 1:1 key — peer `AgentKey`, not the `XOR` id (the A2 open question).**
`ChatSessionKind::Direct` stores the **peer `AgentKey`**, because:

- It is exactly what the *typed* IM surface already hands us — inbound
  `InstantMessageReceived(Box<InstantMessage>)` has `from_agent_id`, outbound
  `send_instant_message` takes `to_agent_id` — so finding/opening a 1:1 session
  needs no XOR math on the common paths.
- It is human-meaningful, stable (a conversation is "with this avatar"), which
  the opaque `XOR` `Uuid` is not.
- The `XOR` `ImSessionId` stays fully available: `compute_im_session_id`
  (`session/conversions.rs:808`) derives it on demand for wire correlation, and
  because byte-wise XOR is **self-inverse**, a wire-only 1:1 signal that arrives
  keyed by the `XOR` id — **`ImTyping`** carries `session_id` = the `XOR` id for
  1:1 — maps **back** to the peer. A small helper
  `direct_peer_from_session_id(agent_id, session_id) -> AgentKey` does the
  reverse XOR, mirroring `compute_im_session_id`'s self-IM special-case (if
  `session_id == agent_id.uuid()` the peer is the agent itself; else
  `peer = XOR(agent_id, session_id)`).

**Mapping each inbound event to a registry key** (the keying is total — every
session-bearing event resolves to exactly one `ChatSessionKind`):

| Inbound event | Field used | Key |
|---------------|-----------|-----|
| `InstantMessageReceived` (dialog `Message`) | `from_agent_id` | `Direct { peer }` |
| `ImTyping` (1:1) | `session_id` (`XOR`) → reverse-XOR | `Direct { peer }` |
| `GroupSessionMessage` / `GroupSessionParticipant` | `group_id` | `Group { group_id }` |
| `ConferenceSessionMessage` / `ConferenceSessionParticipant` | `session_id: Uuid` → `ImSessionId::from` | `Conference { id }` |
| `ConferenceInvited` | `session_id` + `from_group` | `Group`/`Conference` (A5) |

(Existing events carry a raw `Uuid`/`GroupKey` `session_id`; the registry wraps
each into the typed key on lookup — see the A1 note on retrofitting those event
fields, optional, A10.)

**The value — `ChatSession`.** Carries only mutable per-session state (the kind
is the key), each field tagged with the item that fills it:

    struct ChatSession {
        /// Live roster for group/conference (A6: SessionAdd/SessionLeave).
        /// Empty/implicit `{ self, peer }` for Direct — not materialized.
        participants: BTreeSet<AgentKey>,
        /// Who is currently typing in this session (A6: ImTyping / Typing*).
        typing: BTreeSet<AgentKey>,
        /// Monotonic time of the last message / typing / roster change (A2).
        last_activity: Instant,
        // history + unread / last_read: added by A8 (bounded log + marker).
        // invite status (pending / joined): added by A5.
    }

- **`participants` / `typing`** — reserved here as `BTreeSet<AgentKey>` (typed
  keys), folded by **A6**. For **Direct** the roster is implicitly
  `{ self, peer }` (peer is in the key) and `SessionAdd`(13) / `SessionLeave`
  (18) do **not** apply; for **Group / Conference** the roster is seeded/updated
  from `GroupSessionParticipant` / `ConferenceSessionParticipant` (`joined` =
  insert / remove). A2 fixes the field + type + source; A6 owns the fold.
- **`last_activity: Instant`** — the **only** field A2 fills. Stamped to
  the passed-in `now` (the crate's sans-IO clock; `Instant`, as
  `sit`/`teleport`/circuit timers use) on every message / typing / roster
  change. Drives display ordering and any future idle handling; it **never**
  drives presence (A3 — presence comes only from the authoritative
  notifications).
- **history / unread (A8)** and **invite status (A5)** are deliberately **not**
  added by A2 — the struct grows additively as those items land, so each can
  pick its own representation without reworking A2.
- **No `Default`** — `Instant` has no `Default`; the value is built by
  `ChatSession::new(now)` (empty sets, `last_activity = now`).

**Lazy open — the get-or-create primitive.** A private helper

    fn chat_session_mut(&mut self, kind: ChatSessionKind, now: Instant)
        -> &mut ChatSession

does get-or-create: `entry(kind).or_insert_with(|| ChatSession::new(now))`, then
stamps `last_activity = now`, and returns the entry for the caller to mutate. A
read-only `chat_session(kind) -> Option<&ChatSession>` does **not** create.
**1:1** sessions open on the first inbound *or* outbound 1:1 `Message` IM under
the `Direct` key. *Which* event opens *which* kind beyond that — does an inbound
`GroupSessionMessage` open a group session, or only the outbound
`start_group_session`? does a `ConferenceInvited` open a pending entry? — is
**A4's** lifecycle decision (and A5's for invites); A2 supplies the single
storage primitive they all call so the open semantics stay in one place.

**Persistence & reset (preview; owned by A9/A7).** `chat_sessions` is
**grid-level** and is **not** cleared at the `SitState`/teleport reset sites
(`begin_handover`, `TeleportLocal`, `promote_child_to_root`) — the *inverse* of
the seat/permission reset; it clears only on logout (A9). A7's presence-driven
auto-reset *mutates* entries (clears `typing`, drops a friend from rosters,
closes the 1:1 whose peer went offline) but is the only path that removes a
session short of logout. A2 only notes this; the hooks are A7/A9.

**Accessors (read model; registry types stay private).** A2 reserves the
registry accessor; the full read surface (participants / typing / history /
unread) is A10's API delta. The session list is exposed as a public view
assembled from `(key, value)` — a `ChatSessionInfo` flattening the
`ChatSessionKind` + the public state — never leaking `ChatSession` /
`BTreeMap` internals (the `ScriptGrantInfo` precedent). Names finalized in A10.

### B2. Chat-session registry (storage + keying) (from A2)

Build the registry skeleton B1's `ChatSessionKind` keys, with no wire folding
yet (that arrives with A4/A6's tasks). Concretely:

- Add `ChatSession` (struct: `participants` / `typing: BTreeSet<AgentKey>`,
  `last_activity: Instant`) with `ChatSession::new(now)` — **not** `Default`
  (`Instant` has none). History / unread (A8) and invite status (A5) are added
  by those tasks, not here.
- Add the private field `chat_sessions: BTreeMap<ChatSessionKind, ChatSession>`
  to `Session` (`session.rs`), beside `sit` / `teleport`; give
  `ChatSessionKind` the `Ord` derive B1 left to A2 (it **is** the map key).
- Add the get-or-create helper `chat_session_mut(kind, now)` and the read-only
  `chat_session(kind)`, plus the reverse-XOR helper
  `direct_peer_from_session_id(agent_id, session_id)` (self-IM special-case
  matching `compute_im_session_id`).
- **No** inbound folding, **no** command/event, **no** reset hook in B2 — those
  are later tasks (A4 lifecycle, A6 rosters/typing, A7 reset, A9 persistence,
  A10 accessors). B2 is the storage + keying skeleton.
- Unit tests: insert/lookup under each `ChatSessionKind`; `chat_session_mut`
  creates once and re-fetches (stamping `last_activity`); the reverse-XOR
  (`direct_peer_from_session_id(a, compute_im_session_id(a, b)) == b`)
  including the self-IM case.

This task stays **drafted/blocked** until Phase A is signed off; it builds on B1
(and may absorb it at the Phase B consolidation).

### Friend-presence reference (from A3)

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

### B3. Friend-presence cache (buddy list + online set) (from A3)

Add the two presence stores and wire them into the existing handlers, plus the
live friendship-add paths. The only API change is one new field on
`accept_friendship` / `Command::AcceptFriendship`; no new event:

- Add `friends: BTreeMap<FriendKey, Friend>` + `online: BTreeSet<FriendKey>` to
  `Session` (`session.rs`), beside `chat_sessions` / `sit` / `teleport`.
- Seed `friends` at the `FriendList` site (`methods.rs:1078`) from the same
  `friend()`-mapped data; leave `online` empty at login.
- Fold each existing handler (record **in addition to** emitting its event):
  `OnlineNotification` (`:3504`) inserts into `online`; `OfflineNotification`
  (`:3514`) removes; `ChangeUserRights` (`:3524`) updates the cached `Friend`'s
  rights by `granted_to_us` (ignore if absent); `TerminateFriendship` (`:2586`)
  removes from both stores.
- **Live friendship add (both directions):** in the inbound IM dispatch, on
  `ImDialog::FriendshipAccepted`, insert `from_agent_id` into `friends` with
  default `CAN_SEE_ONLINE` both ways (still emit `InstantMessageReceived` —
  surface unchanged); and **add a `friend_id: FriendKey` field** to
  `Command::AcceptFriendship` + a param to `accept_friendship`, inserting
  the friend on accept with the same default. Wire the new command field through
  `sl-client-tokio` / `sl-client-bevy` / the REPL at parity (the driver fills it
  from the `FriendshipOffered` IM it is answering).
- Accessors `friends()` / `friend(id)` / `is_online(id)` / `online_friends()`
  returning the public `Friend` / `bool` / `FriendKey`.
- **Invariant:** no IM / chat-session path mutates `online` — assert this in
  a test (deliver an IM after an `OfflineNotification`; the peer stays offline).
  (The `FriendshipAccepted` add touches `friends`, never `online` — presence for
  the new friend still arrives via its own `OnlineNotification`.)
- **No** A7 chat-reset here (B-task A7) and **no** persistence/close hook
  (A9).
- Unit tests: `FriendList` seeds the cache (and `online` empty); online/offline
  insert/remove; rights change in each direction mutates the right field; an
  unknown-friend rights change ignored; `TerminateFriendship` drops from both;
  **a `FriendshipAccepted` IM adds the friend (default rights), and
  `accept_friendship(friend_id, …)` adds the friend** — both live, no relogin;
  the IM-after-offline invariant above.

This task stays **drafted/blocked** until Phase A is signed off; independent
of B1/B2 (presence is a separate store from the chat-session registry) and may
land alongside them.

### Session-lifecycle reference (from A4)

The state machine over the A2 `chat_sessions` registry: how each kind opens,
what "joined" means without a UDP ack, and what removes an entry. A4 adds one
field to `ChatSession` and wires the transitions into the *existing* outbound
methods and inbound handlers — no new command (A5 adds accept/decline). The
simulator stays authoritative; the lifecycle is an optimistic local mirror.

**The lifecycle field** (on `ChatSession`, the A2-deferred "invite status" slot,
now generalised):

    enum ChatSessionLifecycle { Invited, Joined }

- **`Joined`** — we believe we are an active participant. This is the state for
  **every 1:1** (the moment it opens), a group/conference we **started**, one
  we **accepted** an invite to, and any session we have seen **inbound traffic**
  in. It is **optimistic**: there is **no UDP "joined" ack** (the modern CAPS
  `ChatSessionRequest` would return one, but it is not implemented — § Protocol
  reality), so `Joined` means "we acted / saw traffic", not "sim-confirmed".
- **`Invited`** — a conference/group invite we have **not** acted on and have
  seen **no** traffic for. Set **only** by the A5 invitation path
  (`Event::ConferenceInvited`). A bare invite is the *one* non-`Joined` case.

1:1 never carries `Invited` (there is no IM invitation — you just message and it
opens). `chat_session_mut` (A2) creates with **`Joined`** by default (the common
"opened by our action / by traffic" case); A5's invite-create is the sole path
that overrides the new entry to `Invited` before any traffic.

**Open / join transitions** (each maps onto a real site; the inbound rows share
the handler A6 folds rosters into and A8 folds history into — B4 adds only the
get-or-create + `lifecycle = Joined` stamp there):

| Trigger | Kind | Effect |
|---------|------|--------|
| First inbound *or* outbound 1:1 `Message` IM | Direct | get-or-create, `Joined` |
| `start_group_session` (outbound) | Group | get-or-create, `Joined` |
| inbound `GroupSessionMessage` / `GroupSessionParticipant` | Group | get-or-create, `Joined` (promotes `Invited`) |
| `start_conference` (outbound) | Conference | get-or-create, `Joined` |
| inbound `ConferenceSessionMessage` / `ConferenceSessionParticipant` | Conference | get-or-create, `Joined` (promotes `Invited`) |
| `ConferenceInvited` (no traffic yet) | Conf / Group | get-or-create, `Invited` (A5) |
| accept invite (A5 command) | Conf / Group | `Invited` → `Joined` (+ implicit-join send) |

- **Inbound group/conference traffic opens & tracks the session** (the A4 open
  question — answered **yes**). The sim routes a group/conference IM only to a
  participant, so receiving one means we are effectively in it (e.g. auto-joined
  group chat after login, or a conference we were added to). This matches the
  viewer opening a session tab on the first inbound message, and it **promotes**
  any pre-existing `Invited` entry to `Joined`.
- **Promotion rule:** any session message / participant event sets
  `lifecycle = Joined` on the (get-or-created) entry — so an `Invited` that
  later sees traffic becomes `Joined` without an explicit accept (you joined by
  traffic). A4 needs no separate "joined ack" because traffic *is* the signal.
- **Optimism caveat:** if a `start_group_session` fails (e.g. not a member) the
  sim replies with an error event, not a session-close; the entry stays `Joined`
  until the driver removes it. Surfacing that error is app policy, out of
  A4's scope.

**Leave / close / remove transitions:**

| Trigger | Kind | Effect on `chat_sessions` |
|---------|------|---------------------------|
| `leave_group_session` / `leave_conference` (outbound) | Group / Conf | **remove** the entry |
| decline invite (A5 command) | Conf / Group | **remove** the `Invited` entry |
| logout (`SessionState::Closed`) | all | all cleared (constructor rebuild) |
| 1:1 — *no leave op exists* | Direct | never removed (persists to logout) |

- **Explicit leave removes** — the registry tracks *current* sessions; once we
  send `SessionLeave` we are out, so the entry goes. (If retaining a left
  session's log is later wanted, that is an A8 history-retention call; A4 keeps
  the registry to live sessions.)
- **1:1 has no leave** — there is no `SessionLeave` for a direct IM; a 1:1 entry
  lives until logout. A7's peer-offline handling may **mark/close** a 1:1
  (a lifecycle/annotation change A7 defines) but **never removes** it, so its
  history survives the peer going offline.
- **No `close` hook** — a `Closed` session is dead and a relogin rebuilds the
  registry through the constructor, as A2/A9 decided for the chat stores;
  A4 adds no logout-time clearing code.

**No new command.** The outbound lifecycle surface already exists —
`StartGroupSession` / `SendGroupMessage` / `LeaveGroupSession`,
`StartConference` / `SendConferenceMessage` / `LeaveConference`,
`InstantMessage` (A1 inventory). A4 only hooks the registry transitions into the
methods behind them; the **accept/decline** commands (the only genuinely new
lifecycle verbs) are A5's, because they are inseparable from the invitation
model. A4's accessor contribution is the `lifecycle` exposed on the A10
`ChatSessionInfo` view.

### B4. Chat-session lifecycle transitions (from A4)

Add the lifecycle state and wire the open/join/leave/remove transitions, with no
new command:

- Add `enum ChatSessionLifecycle { Invited, Joined }` and a `lifecycle` field on
  `ChatSession` (fills the A2-reserved "invite status" slot); `ChatSession::new`
  defaults it to `Joined`.
- **Outbound:** in `start_group_session` / `start_conference`, get-or-create the
  `Group` / `Conference` session as `Joined`; in `send_group_message` /
  `send_conference_message` / `send_instant_message`, get-or-create (1:1 opens
  here) and stamp `Joined`; in `leave_group_session` / `leave_conference`,
  **remove** the entry.
- **Inbound:** in the `GroupSessionMessage` / `ConferenceSessionMessage` and the
  participant handlers, get-or-create + set `lifecycle = Joined` (promoting any
  `Invited`). This is the **same** call site A6 (rosters/typing) and
  A8 (history) extend — they compose; B4 adds only the lifecycle stamp.
- **No** `Invited`-creation here (that is A5's invitation task), **no** accept /
  decline command (A5), **no** logout hook (A9).
- Unit tests: outbound `start_*` creates `Joined`; an inbound group/conference
  message opens a `Joined` session and promotes a pre-seeded `Invited` one;
  `send_instant_message` / inbound 1:1 opens a `Joined` Direct session;
  `leave_*` removes; a 1:1 is never removed by any leave path.

This task stays **drafted/blocked** until Phase A is signed off; it builds on B2
(the registry) and shares inbound handler sites with the A6 / A8 tasks.
