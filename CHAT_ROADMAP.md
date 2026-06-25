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
- Inbound offline IMs already surface (`offline = true`), but **offline-IM
  history retrieval** (`ReadOfflineMsgs` CAPS / the legacy
  `RetrieveInstantMessages` UDP) is **not** implemented.
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

- [ ] **A1. Inventory the surface & define the unified model.** Enumerate the
  three chat kinds and their id derivation (1:1 `XOR`, conference caller-minted
  `ImSessionId`, group = group id); the command / method / event / `ImDialog`
  surface; the two delivery paths (UDP `ImprovedInstantMessage` vs CAPS
  `ChatterBoxInvitation`); and the friend-presence surface (`FriendList`,
  `FriendsOnline` / `FriendsOffline`, `FriendRightsChanged`; friends-only,
  `CAN_SEE_ONLINE`-gated, passive). Define a unified `ChatSession` concept
  (`Direct { peer } | Group { group_id } | Conference { id }`) and a
  presence/buddy concept, and state the boundary (local chat is OUT).
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

Phases B+ (implementation) are defined once Phase A's design decisions are
signed off; this document scopes the planning only.
