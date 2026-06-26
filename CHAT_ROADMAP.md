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
- [ ] **A2. Design the chat-session state model & keying.** Specify what
  `Session` stores (beside `TeleportPhase` / `SitState`): a registry keyed by
  the canonical session id → `ChatSession { kind, participants, typing,
  last_activity, unread / last_read, invite status }`. Decide how 1:1 sessions
  are lazily opened (on the first inbound/outbound IM under the `XOR` id), the
  participant source (`SessionAdd` / `SessionLeave`), and whether the 1:1 key
  stores the peer `AgentKey` or the `XOR` `ImSessionId`.
- [ ] **A3. Design the friend-presence state model.** A buddy-list cache
  (`Friend { id, rights_granted, rights_received }`) and an online set keyed by
  `FriendKey`, seeded by `FriendList` at login and updated by `FriendsOnline` /
  `FriendsOffline` and `FriendRightsChanged`. Presence is friends-only /
  `CAN_SEE_ONLINE`-gated / passive. Drive the online set **only** from the
  authoritative presence notifications (and the login buddy list) — never infer
  presence from IM send/receive activity. (Known reference-viewer / SL-grid bug
  to **avoid replicating**: an IM sent immediately after a peer goes offline
  falsely re-marks them online; this design must ignore IM traffic as a presence
  signal.) Accessors: `friends()`, `is_online(friend)`, `online_friends()`.
- [ ] **A4. Design the session lifecycle (open / join / send / leave / close).**
  1:1 implicit on the first message; group via `start_group_session` (decide
  whether an inbound group message also opens/tracks it); conference via
  `start_conference` (caller mints the id) or via accepting an invite. Define
  what marks a session *active/joined* versus *pending* (there is no UDP
  "joined" ack) and what removes it from the registry (an explicit leave,
  logout).
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
