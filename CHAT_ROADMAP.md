# chat session road map

A plan to give the SL client a *stateful* chat-session system covering the three
instant-message session kinds — 1:1 direct IM, ad-hoc conferences, and group
chat — each potentially carrying **both a text and a voice channel** (voice at
the SL *signalling* level only — A12), with **friend presence** folded in. Today
this whole surface is a
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
  **separate** concern, **out of scope** for the session-**state** model here
  (this roadmap is about IM / conference / group **sessions**) — but it **is**
  included in the optional **chat-log files** (A13), which cover *all* text-chat
  types (nearby + IM + group + conference).
- Optional **local chat-log files** (write + read-back, Firestorm-style; A13)
  in scope for **long-term** history beyond the in-memory cap; this is a
  **runtime** file-I/O feature (the sans-IO `Session` does no I/O), default off.
- A session's **voice channel is in scope at SL-signalling level** (has-voice,
  channel info, join/leave-voice, voice membership — A5 / A12), reusing the
  existing voice-signalling feature. The **Vivox/WebRTC audio transport and the
  "who is speaking" / talk-activity indicators are OUT of scope** (the external
  voice client's job); sl-client models voice *state*, not voice *audio*.
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
  field A2 fills), with history/unread (A8), the lifecycle (A4, enriched A5)
  and voice-channel state (A12) added additively. Lazy get-or-create via
  `chat_session_mut(kind, now)`; *which* event opens *which* kind is A4's
  lifecycle call.
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
- [x] **A4. Design the session lifecycle (open / join / send / leave / close).**
  1:1 implicit on the first message; group via `start_group_session` (decide
  whether an inbound group message also opens/tracks it); conference via
  `start_conference` (caller mints the id) or via accepting an invite. Define
  what marks a session *active/joined* versus *pending* (there is no UDP
  "joined" ack) and what removes it from the registry (an explicit leave,
  logout).
  **Done — see § Session-lifecycle reference (from A4) + tasks B2 / B5 in
  § Phase B.**
  Decided a `lifecycle: ChatSessionLifecycle { Invited | Joined }` field on
  `ChatSession` (this *is* the A2-deferred "invite status"; **A5 later enriches
  `Invited` to `Invited(PendingInvite { …, channel })`** to carry the invite +
  its text/voice channel). This `lifecycle` is the **session-level** membership
  (driven by the *text* channel / our actions); the **voice** channel's
  join-state is a *separate* A12 facet on the same session, so the two never
  conflict. **1:1 is always
  `Joined`** the instant it opens (no handshake); **group / conference open
  `Joined` optimistically** on our `start_*`/accept *or* on **any inbound
  message/participant traffic** (yes — an inbound group/conference message opens
  & tracks the session, promoting an `Invited` entry to `Joined`); **`Invited`**
  is set *only* by a bare invitation with no traffic yet (A5 feeds it). On the
  **UDP** path there is **no "joined" ack**, so `Joined` is *optimistic*; on the
  **CAPS** path A5's `"accept invitation"` reply confirms it. **Removal:** an
  explicit `leave_group_session` /
  `leave_conference` **removes** the entry; an A5 decline removes the `Invited`
  entry; **logout** clears all (constructor rebuild, no `close` hook — the
  A2/A9 convention). **1:1 is never removed** by a leave (no such op) — it
  persists to logout (A7 may *mark* it on peer-offline, never remove). No new
  command (the start/send/leave surface already exists; A5 adds accept/decline).
- [x] **A5. Design invitation handling + accept/decline.** A pending-invitations
  registry fed by `Event::ConferenceInvited` (and group invites), plus new
  accept/decline commands. Decide the path: adopt the modern
  `ChatSessionRequest` capability (its accept-invitation method; not implemented
  today) versus the UDP implicit-join. Output: the invitation lifecycle and the
  new command(s).
  **Done — see § Invitation-handling reference (from A5) + B5 in § Phase B.**
  **Scope (user-set): a chat session can carry both a TEXT and a VOICE channel
  (a group/conference has both), so this roadmap handles *both* — invitations
  come in text and voice flavours and A5 covers each.** Decisions: pending
  invitations are the A4 **`Invited` entries** enriched to
  `Invited(PendingInvite { inviter, session_name, channel: InviteChannel })`
  where `InviteChannel { Text | Voice | Both }` (from the `ChatterBoxInvitation`
  body — `instant_message` vs `voice`). Commands `AcceptChatInvite` /
  `DeclineChatInvite { session_id, from_group }`. The modern path is the shared
  **`ChatSessionRequest`** cap; **text and voice use *different* methods on it**
  (the distinction that matters): join/leave **text** = `"accept invitation"` /
  `"decline invitation"` (the `"accept invitation"` reply is the **participant
  roster** → feeds A6); a **voice** accept additionally **starts the voice
  channel** (the existing voice feature), a voice decline is `"decline
  invitation"` for multi-agent or `"decline p2p voice"` for 1:1 — A5 uses the
  text methods for the text channel and the voice methods for the voice channel,
  never conflating them. **UDP fallback** (OpenSim stubs `ChatSessionRequest` in
  its voice modules): text accept = optimistic `Joined` (sim already added us),
  text decline = `SessionLeave`; OpenSim voice is its own FreeSwitch/Vivox path.
  Sans-IO `Session` always does the registry transition (accept →
  `Joined`; decline → remove). The per-session **voice-channel state**
  (has-voice, `voice_channel_info`, joined-voice at *signalling* level, voice
  membership from the SL roster) is the new **A12** (appended below); A5 only
  feeds the invite→join-signalling trigger. **Out of scope (user-set):** the
  Vivox/WebRTC audio transport itself and the "who is speaking" indicators it
  drives — those live in the external voice client, not sl-client (whose voice
  feature is SL *signalling* only). Note the **decoder gap**:
  `chatterbox_invitation_from_llsd` does not yet read the `voice` body, so B5
  must classify the invite's `InviteChannel`. (1:1 *text* has no invite; a 1:1
  *voice* call is a P2P voice invite, in scope at the signalling level.)
- [x] **A6. Design participant & typing tracking.** From
  `Group` / `ConferenceSessionParticipant` and `ImTyping`, maintain per-session
  rosters and a per-session typing set; define accessors
  (`participants(session)`, `typing(session)`). Decide how outbound
  `send_im_typing` interacts and whether typing entries auto-expire.
  **Done — see § Participant & typing reference (from A6) + task B3 in
  § Phase B.** Decided: the **roster** (A2's `participants: BTreeSet<AgentKey>`)
  is folded from `Group`/`ConferenceSessionParticipant` (`joined` =
  insert/remove) at the existing dispatch sites — via `chat_session_mut`, so a
  participant event *also* opens the session (the A4 rule) — and **seeded
  from the A5 CAPS accept-reply roster**; 1:1 is not materialized (the accessor
  synthesises `{ peer }` from the key). **Typing** refines A2's field to
  `typing: BTreeMap<AgentKey, Instant>` (last-seen) for **auto-expiry**: folded
  from `ImTyping` (`true` = insert/refresh `now`, `false` = remove), keyed by
  the **typer** for 1:1 (`Direct { from_agent_id }` — robust, no reliance on the
  wire `id`) or the matching `Group`/`Conference` session otherwise; **typing
  never opens a session** (ephemeral). **Auto-expiry = yes**, after a
  `TYPING_TIMEOUT` of **9 s** (Firestorm `OTHER_TYPING_TIMEOUT`; senders refresh
  ~4 s), pruned in `poll(now)` so the accessor needs no `now`; an explicit
  `TypingStop` clears immediately. Outbound `send_im_typing` tracks **nothing**
  (the set is remote typers only; our own typing is outbound). A7 also clears an
  offlined friend's typing/roster — it layers with expiry. Accessors
  `participants(session)` / `typing(session)`.
- [x] **A7. Design presence-driven auto-reset.** On `FriendsOffline`, for each
  offlined friend: clear their typing in every session; mark/close the open
  **1:1** session whose peer is that friend; and best-effort update **conference
  / group rosters** where they appear as a participant (drop or mark-left).
  State the caveat explicitly: presence is friends-only, so this only covers
  friend-participants who grant see-online — **non-friend** participants still
  rely on the simulator's `SessionLeave` events. The two signals layer; they do
  not replace each other. On `FriendsOnline`: update the presence set (and
  optionally clear a stale "peer offline" marker on the 1:1 session); no other
  auto-action. Define the exact session transitions.
  **Done — see § Presence-driven reset reference (from A7) + task B6 in
  § Phase B.** Decided: on `FriendsOffline`, at A3's `OfflineNotification`
  handler where A3 removes the friend from `online`, for each offlined agent
  iterate every `ChatSession` and **remove that agent from `typing` and from
  `participants`** (the 1:1's `participants` is unmaterialised, so only its
  `typing` is touched). **No session is removed and no per-session "offline"
  marker is stored** — refining "mark/close": a 1:1 is never removed
  (A4), and its peer-offline state is already `!is_online(peer)` from the A3 set
  (single source of truth — a stored marker would duplicate it). So
  **`FriendsOnline` needs *no* chat action** (no marker to clear; the friend
  re-joins via `SessionAdd`/messages) — A3's set-add is the whole effect.
  **Caveat:** presence is friends-only / see-online-gated, so the roster
  drop covers only friend-participants who grant see-online; **non-friend**
  participants are dropped solely by the sim's `SessionLeave` (A6). The two
  **layer** — A7 is the fast path for friends (also covers a crash with no
  `SessionLeave`), `SessionLeave` covers everyone; both idempotent. Typing is
  also cleared by the A6 9 s expiry — A7 just does it immediately. No new event
  (the driver already gets `FriendsOffline`).
- [x] **A8. Design message history, unread & offline retrieval.** Plan a bounded
  per-session message log (sender, timestamp, text, dialog), an unread /
  last-read marker per session, and offline-IM retrieval — the modern
  `ReadOfflineMsgs` CAPS (and/or the legacy `RetrieveInstantMessages` UDP),
  neither implemented yet. Decide retention bounds (cap the log length), what
  counts as unread, and how login drains queued offline IMs into the right
  sessions.
  **Done — see § History & unread reference (from A8) + task B4 in § Phase B.**
  **Correction:** offline-IM *retrieval* already ships (A1) — both
  `Command::RetrieveInstantMessages` and `Command::RequestOfflineMessages` —
  so A8 plans only the **bounded log + unread** model and how the replayed IMs
  **drain** into sessions. Decided: `ChatSession` gains `history:
  VecDeque<ChatMessage>` (cap `HISTORY_CAP = 256`, pop-front oldest; in-memory
  only per A9) and `unread: u32`, where `ChatMessage { sender: AgentKey, dialog:
  ImDialog, text: String, timestamp: Option<u32> }` (the wire Unix `timestamp` —
  `None` for our own sends, since sans-IO has no wall-clock; insertion order is
  the sequence). **Logged dialogs:** only conversation — inbound 1:1 `Message`
  (incl. offline replays) and group/conference `SessionSend`, plus **our own
  outbound** IM/group/conference sends (`sender = self`); typing / participant /
  offers / notices / `FromTask` are **not** logged. **Unread:** `+1` per inbound
  message from another agent (offline IMs included); our own outbound resets it;
  a new `mark_session_read` (`Command::MarkSessionRead`) also resets. **Offline
  drain** is automatic: replayed `offline = true` IMs flow through the same
  inbound logging — opening the `Direct { from_agent_id }` session (A4), append
  with the original wire `timestamp`, bumping `unread` — so login populates
  the right sessions once retrieval is fired (driver/runtime policy; viewers
  auto-request at login). Accessors `history(session)` / `unread(session)` /
  `total_unread()`.
- [x] **A9. Lock the persistence-vs-region behaviour.** Chat sessions,
  history, and presence are **grid-level** and **persist** across teleport
  (`begin_handover`, `TeleportLocal`), neighbour crossing
  (`promote_child_to_root`), and `DisableSimulator` — explicitly **not** reset
  (the inverse of the `SitState` reset at those same sites). It is not cleared
  even on logout — it survives into the `Closed` state so the final pre-logout
  conversation stays readable, vanishing only when the `Session` is dropped
  (revised 2026-06-27, below). Persistence **beyond** a single session **is** in
  scope — the optional local chat-log files (**A13**); the sans-IO `Session`
  state itself stays in-memory and A13's *runtime* file layer is the long-term
  store. A9 locks the in-memory region-behaviour; A13 owns the on-disk
  behaviour.
  **Done — see § Persistence & region reference (from A9) + task B10 in
  § Phase B.** Decided: the three chat/presence stores (`chat_sessions` /
  `friends` / `online`) are **grid-level**, so — unlike `sit` / `script_grants`,
  which are reset at the four region-boundary sites — they are touched at
  **none** of `begin_handover` (`methods.rs:760`, which resets `sit` at `:800`
  and drops in-world grants at `:803`), `promote_child_to_root` (`:897`),
  `TeleportLocal` (`:2237`, `sit` reset at `:2243`), or the child-circuit
  `DisableSimulator` (`:1206`). The **rule is "add no clear at those sites"** —
  there is no positive code to write, only the guard that B2/B1's stores are
  never wired into those handlers. **Logout never clears them either — they
  survive into the `Closed` state for post-logout inspection** (revised
  2026-06-27 on user request: a user may still want to read the messages from
  immediately before logout). `close` (`:9599`) / `LogoutReply` (`:3548`) / the
  logout-timeout (`:3597`) only set `SessionState::Closed` (terminal —
  `is_closed`, `:9594`) and emit the disconnect event; **no field is cleared in
  place**, so the read accessors (`history` / `chat_sessions` / `friends` /
  `is_online`) stay valid on a closed `Session` and the final
  conversation/roster/presence remain readable until the driver **drops** the
  struct. The stores vanish only by that **discard**: a relogin builds a
  **fresh** `Session::new(login)` (`:151`, a `const fn`) whose stores start
  empty — so **no `close` hook, no reset code** (the A2/A3 convention, now
  doubly justified: clearing on close would *destroy* the post-logout history
  the user wants). The chat fields slot into the `const fn` constructor beside
  `sit: SitState::NotSitting` (`:165`) / `script_grants: BTreeMap::new()`
  (`:167`) as `chat_sessions` / `friends: BTreeMap::new()` + `online:
  BTreeSet::new()` — all const-constructible, no `const fn` regression. B10 is a
  **verification + guard** task: the cross-region persistence tests are the
  **inverse** of `teleport_clears_seat` (`tests/lifecycle.rs:1716`) — after a
  teleport / crossing / `DisableSimulator`, a seeded chat session / history /
  roster / presence entry must **still** be present. Cross-session (relogin)
  persistence is **out** of the in-memory scope — that is A13's optional on-disk
  file layer.
- [x] **A10. Specify the API-surface delta & driver/REPL exposure.** Enumerate
  the new/changed `Command`s (accept/decline invitation, an optional open/close
  session, request offline IMs), any `Event` changes, and the new `Session`
  accessors (`sessions()`, `session(id)`, participants, typing, history, unread,
  `friends()`, `is_online`); and how `sl-client-tokio`, `sl-client-bevy`, and
  the REPL expose them at feature parity. Draw the boundary between sl-proto
  `Session` state and application policy.
  **Done — see § API-surface & exposure reference (from A10) + task B7 in
  § Phase B.** Decided (the exposure model is the load-bearing call, refined
  with the user 2026-06-27 around chat-history scale): the sans-IO `Session`
  keeps **public read accessors** as the primary, zero-copy API, but how the
  *runtimes* surface them **diverges by runtime, on purpose** — a refinement of
  the strict "all reads via `Event`" PERMISSION rule. **bevy** holds the
  `Session` in a Resource, so its systems read the read model by **direct
  `&Session` borrow** (true zero-copy, no `Arc`, no query command). **tokio**'s
  `Client::run(self, …)` (`lib.rs:269`) **consumes** the Client into a spawned
  task (`tokio::spawn(client.run(…))`, `sl-repl-tokio:587`), leaving the app
  only the command/event channel ends, so tokio **and the REPL** use a **pull
  bridge**: query `Command`s → synthesized reply `Event`s (the
  `QueryScriptPermissions` → `ScriptPermissionState`, `methods.rs:5739`
  / tokio `:1191` / bevy `:1963`), but the replies carry **`Arc<[…]>`**
  snapshots / **paged** windows, never deep `Vec` copies. **Parity is redefined:
  identical data + identical `Command`s + identical view types across all three
  runtimes; only the read *transport* differs** (bevy borrow vs tokio/REPL
  pull). **History-scale design** (the user's `chat.txt` is 3.8M lines, largest
  IM 120k): the in-memory `Session` holds only the A8 **256-cap hot tail**,
  never the archive; the deep archive is **A13's on-disk** file layer, read **on
  demand a page at a time** (cursor + `limit`, file `seek` / `mmap`) so only a
  screenful ever crosses regardless of total size; the bounded tail is
  **`Arc<[ChatMessage]>`-shared** (O(1) hand-off; bevy borrows it). Bulk
  history is therefore **never wholesale-copied**. New **read** surface (B7):
  `QueryChatSessions` → `Event::ChatSessions(Arc<[ChatSessionInfo]>)` (light:
  kind / lifecycle + pending-invite / participants / typing / unread, **no
  history**); `QueryChatHistoryPage { session, before: Option<MessageCursor>,
  limit }` → `Event::ChatHistoryPage { session, messages: Arc<[ChatMessage]>,
  prev }`; `QueryFriends` → `Event::FriendsSnapshot(Arc<[FriendPresence]>)`;
  snapshot-builder accessors `chat_sessions_info()` / `friends_presence()` on
  `Session` (composing the B2/B1/B3/B4 accessors) and the new public views
  `ChatSessionInfo` / `FriendPresence` / opaque `MessageCursor` (`ChatMessage`
  already public, B4). The existing inbound events (`InstantMessageReceived` /
  `ImTyping` / `Group`·`ConferenceSession*` / `Friends*`) **double as
  change-notifications** — no new push event. **Action** commands are unchanged
  here and stay full-parity (the 6-site pattern): `AcceptChatInvite` /
  `DeclineChatInvite` (B5), `MarkSessionRead` (B4), and the **changed**
  `AcceptFriendship { …, friend_id }` (B1). **Boundary:** sl-proto owns the
  in-memory read model + optimistic lifecycle + wire + the snapshot-builders;
  **app/runtime owns policy** — when to fire offline-IM retrieval (login),
  auto-accept-text vs prompt-for-voice, when to `MarkSessionRead`, the
  CAPS-vs-UDP accept/decline path (runtime owns caps + HTTP — B5), and the
  A13 file layer (write + paged read-back + serving `QueryChatHistoryPage` /
  bevy's older-page reads). **A12** appends voice fields to `ChatSessionInfo` +
  join/leave-voice commands; **A13** appends the file config + implements
  deep-history paging — both folded at the Phase B consolidation.
- [x] **A11. Define the test & verification strategy.** Plan the
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
- [x] **A12. Design the per-session voice-channel state (signalling only).** A
  chat session (group / conference / 1:1) can carry a **voice** channel beside
  its text channel (user-set scope). Design the SL-**signalling** state the
  `ChatSession` tracks for voice: whether the session *has* voice, the
  `voice_channel_info` (channel uri / credentials handed over on the invite or
  the `"accept invitation"` reply / `ParcelVoiceInfoRequest`), whether we have
  **joined** the voice channel at the signalling level (driven by an A5 voice
  accept), and the voice **membership** (who is in the voice channel, read from
  the SL session roster / agent-list updates — not audio). Reuse the existing
  voice-signalling feature (`Event::VoiceAccountProvisioned`,
  `Event::ParcelVoiceInfo`, `Command::RequestVoiceAccount` /
  `RequestParcelVoiceInfo` / `SendVoiceSignaling`). Add join/leave-voice
  commands at the signalling level and the accessors. **Explicitly OUT of scope
  (user-set):** the Vivox / WebRTC audio transport itself and the
  "who-is-currently-speaking" / talk-activity indicators it drives — those live
  in the external voice client, not sl-client. State the boundary: sl-client
  models voice **session state**, not voice **audio**.
  **Done — see § Per-session voice-channel reference (from A12) + task B8 in
  § Phase B.** voice is a per-`ChatSession` **facet** (the A2/A5-reserved
  "voice-channel state (A12)" slot), **additive** — a `voice: VoiceChannelState`
  field, **not** a separate session. It is **distinct** from the two voice
  surfaces that already exist and **stay standalone**: the **agent-global voice
  *account*** (`VoiceAccountInfo`, provisioned once per login via
  `Command::RequestVoiceAccount` → `Event::VoiceAccountProvisioned`,
  `methods.rs:495`) — the credentials to the voice *server* — and the
  **spatial / parcel channel** (`ParcelVoiceInfo` via
  `Command::RequestParcelVoiceInfo`, `methods.rs:500`) — nearby/local voice,
  which is **not** a chat session (local chat out of scope). A12 adds **only**
  the per-session *room* state for group / conference / 1:1. Shapes:
  `VoiceChannelState { has_voice: bool, channel: Option<VoiceChannelInfo>,
  joined: bool, members: BTreeSet<AgentKey> }` and
  `VoiceChannelInfo { channel_uri: Option<Url>, channel_credentials:
  Option<String>, voice_server_type: Option<String>, session_handle:
  Option<String> }` (mirrors the SL session `voice_channel_info` / the existing
  `ParcelVoiceInfo`'s nested `voice_credentials`, `sl-wire voice.rs:494`).
  **Sources:** `has_voice` / `channel` from A5 voice **invite body** plus the
  `ChatSessionRequest "accept invitation"` reply's `voice_channel_info` (decoder
  gaps — B5 classifies `InviteChannel`, B8 decodes the channel); `joined` set
  **optimistically** by A5 voice-accept / a new `JoinSessionVoice`, cleared by
  `LeaveSessionVoice` / a voice decline (signalling only — no audio ack);
  `members` folded from `ChatterBoxSessionAgentListUpdates` agent-list
  **voice-connected** flag — **NOT** the speaking flag (out of scope). **New
  commands** (full parity): `JoinSessionVoice { session }` /
  `LeaveSessionVoice { session }` — sans-IO records `voice.joined`; **runtime**
  orchestrates the existing voice plumbing (ensure the account via
  `RequestVoiceAccount`, then session voice request via `ChatSessionRequest`;
  leave = `"decline invitation"` / `"decline p2p voice"` per A5, or the WebRTC
  logout `RequestVoiceAccount{logout}` / `SendVoiceSignaling`). **Accessors**
  `session_has_voice` / `session_voice_channel` / `session_voice_joined` /
  `session_voice_members`; the A10 `ChatSessionInfo` gains the reserved voice
  fields (`has_voice` / `voice_joined` / `voice_members`). Voice covers **all
  three kinds** (group / conference multi-agent **and** the 1:1 P2P voice call —
  A5's `"decline p2p voice"`), which **closes A11's open "voice-channel cases"**
  question. **OUT (user-set):** the Vivox / WebRTC audio transport and the
  "who-is-speaking" / talk-activity indicators (the SL-signalling-only scope);
  the boundary is voice **session state**, not voice **audio**. **OpenSim
  limitation:** `ChatSessionRequest` is stubbed and voice runs via
  FreeSwitch / Vivox, so per-session voice is **SL-only testable** (aditi) — the
  same constraint as A5's voice path.
- [x] **A13. Design optional local chat-log files (read + write, all text
  chat).** A **runtime** feature (the sans-IO `Session` does no file I/O — this
  lives in `sl-client-tokio` / `sl-client-bevy`, fed by the event stream) that
  optionally persists message history to per-conversation log files and reads it
  back, for **long-term** scrollback beyond the in-memory A8 cap, **similar to
  the Firestorm viewer** and ideally **format-compatible** with it. **Covers all
  text-chat types** (user-set scope): **nearby / local chat** (`ChatReceived` —
  otherwise out of the session-state scope, but **in** scope for logging), 1:1
  IM, group, and conference. Design, grounded in Firestorm `LLLogChat`
  (`lllogchat.cpp`): a per-account `chat_logs/` directory; per-conversation
  transcript filenames (`chat.txt` for nearby; `firstname.lastname.txt` for 1:1,
  with a legacy display-name option; `<group name> (group).txt` for group; a
  participant-hash name for ad-hoc / conference — sanitised, optional date
  suffix); the line format `[YYYY/MM/DD HH:MM]  Name: message` (timestamp / date
  / seconds toggles; space-prefixed continuation lines); **read-back the tail**
  (Firestorm recalls the last ~20 KB / a "history lines" count) to **seed the A8
  in-memory `history`** on session open; plus the optional `conversation.log`
  metadata index of past conversations. Decide the config surface (enable per
  text-chat type, log dir, filename scheme, timestamp format, recall size),
  default **off** (opt-in, as Firestorm defaults nearby logging off), and how
  the runtime supplies **wall-clock** time (the sans-IO core lacks it — so file
  lines get real dates even for our own sends, A8's `timestamp = None`). Note
  the boundary: A8 is the in-memory working set, A13 is the long-term file store
  that A13 spills to and seeds from.
  **Done — see § Local chat-log files reference (from A13) + task B9 in
  § Phase B.** Decided: A13 is a **runtime** feature (sl-client-tokio /
  sl-client-bevy / REPL at parity) — the sans-IO `Session` does **no** file I/O.
  It taps the **event stream** (+ our own outbound commands) and writes / reads
  per-conversation files; default **off**. Logs **all** text-chat types from
  their events: nearby `Event::ChatReceived` (`event.rs:374` — out of
  session-state scope, logged) → `chat.txt`; 1:1 `InstantMessageReceived`
  → `<account>.txt` (legacy `firstname.lastname.txt` option); group
  `GroupSessionMessage` → `<group> (group).txt`; conference
  `ConferenceSessionMessage` → `Ad-hoc Conference hash<md5-of-sorted-ids>.txt` —
  Firestorm `cleanFileName` sanitisation (``"'\/?*:.<>|[]{}~`` → `_`), optional
  date suffix (`-%Y-%m-%d` nearby / `-%Y-%m` IM·group; never adhoc). Line format
  Firestorm-style — `[YYYY/MM/DD HH:MM:SS]··Name: message` (two spaces after
  `]`; **seconds default ON** — user-set 2026-06-27, vs Firestorm's optional
  default-off `FSSecondsinChatTimestamps`, still format-compatible via its
  `DATE_FORMAT_SEC = "%Y/%m/%d %H:%M:%S"`), toggles timestamp / date / seconds /
  24h, multi-line continuation lines prefixed `\n␠`, colon-in-name → `%3A`,
  system speaker `Second Life:`.
  **Read-back reconciled with A10 (the key correction):** Firestorm seeds the
  in-memory ring from a ~20 KB tail read, but A10's `QueryChatHistoryPage` makes
  the file the **deep archive**, so **B9 serves the *older* pages of
  `QueryChatHistoryPage` directly from the file** (`seek` to the tail / `mmap`,
  parse the window) — **no** sans-IO "seed history" command; the Session ring
  keeps only this-session live messages and on a fresh login *all* scrollback
  comes from the file via paging. Firestorm's `LOG_RECALL_SIZE = 20480` is the
  default seek/page window; a failed line-parse falls back to a plain-text
  message (Firestorm behaviour). Optional `conversation.log` metadata index
  (`[unix] type · · offline name| participant_id session_id history_file|`) for
  conversation discovery, with retention-days purge. **Config** `ChatLogConfig`
  (runtime, default off): enable-per-type (nearby default off), log dir,
  filename scheme (modern / legacy IM names, date-suffix), timestamp format
  (timestamp / date / seconds / 24h), recall window; the per-account dir + names
  come from the runtime's `login_account` (`methods.rs:6959`) / the events'
  `from_name`. **Wall-clock:** A13 is runtime, so it has `SystemTime::now()` —
  file lines get real **local** dates even for our own sends (A8's in-memory
  `timestamp = None`); inbound prefers the wire `timestamp` (Unix; the original
  time for an offline IM) else receipt-now. **Naming correction:**
  A8/B4's history-entry type `ChatMessage` **collides** with the existing
  nearby-chat `ChatMessage` (`types/chat.rs:254`), so it is **renamed
  `SessionMessage`** — threaded through B4 (the entry), A10/B7
  (`ChatHistoryPage { messages: Arc<[SessionMessage]> }`) and B9. **Boundary:**
  A8 = in-memory working set (this session, the 256-cap `history`); A13 =
  long-term on-disk store (all history) and the **only** file I/O — it spills
  A8's messages to disk and serves the archive back through A10's paging. Local
  file I/O is **grid-agnostic**, so A13 is testable on **any** grid (unlike A5 /
  A12 voice). **This closes A11's open "local chat-log file cases" question and
  is the LAST Phase A item — with it Phase A (A1–A13) is complete and signs
  off; Phase B implementation may begin (ask the user first).**

Phase A scopes the planning only; the implementation tasks each Phase A item
produces are appended to **Phase B** below as that item is worked, tagged with
the producing item. Phase B is a *draft* until Phase A is signed off; tick a box
only when the step builds, is clippy-clean (restriction lints), and `cargo test`
passes. Keep `sl-client-tokio`, `sl-client-bevy`, and the REPL at feature
parity; never push client-only types into shared `sl-types`.

## Phase B — design references (from Phase A)

Each Phase A item left a **reference** subsection here recording the design
knowledge it produced. The concrete implementation tasks these imply were
consolidated into **§ Phase B tasks — consolidated (B1–B10)** at the end of this
file (the `PERMISSION_ROADMAP.md` precedent). These reference subsections are
unchanged design records; only their bracketed task tags were re-pointed to the
new B-numbering.

> **Phase A SIGNED OFF — 2026-06-27.** All thirteen design items (A1–A13) are
> complete: every open question is resolved (the A11 table + A12 / A13 closed
> the last three), and the consolidated implementation tasks **B1–B10**
> (§ Phase B tasks — consolidated) are no longer a draft. Phase B **may begin**
> — one task at a time, in dependency order (B1 presence / B2 registry first),
> keeping `sl-client-tokio` / `sl-client-bevy` / the REPL at parity. **Ask the
> user before starting Phase B** (the standing "ask before new roadmap work"
> rule). The consolidation pass that merged the draft B1–B13 into B1–B10 (per
> the `PERMISSION_ROADMAP.md` precedent) is recorded in the note below.
>
> **Phase B consolidated — 2026-06-27.** The draft per-A-item tasks (B1–B13)
> were merged and reordered into the dependency-ordered **B1–B10** in § Phase B
> tasks — consolidated, below, to remove dead-code / rework **between** tasks
> (each task now adds every field/type together with its writer, its reader, and
> tests, so every intermediate commit is clippy-clean under the `unused_*` deny
> lints). The A1–A13 **reference** subsections are unchanged design records.
> Trap → fix summary: draft B1 was a dead type alone → folded into the registry
> (new B2); draft B2 was a dead store (no fold / no accessor) → merged with the
> create/track mechanics (new B2); draft B4's `Invited` variant had no
> constructor until draft B5 → the lifecycle enum is born with the invite task
> (new B5); fields pre-declared before their reader → each field now lands with
> its fold + accessor; `ChatMessage` introduced then renamed to `SessionMessage`
> → the rename is applied up front (new B4); the reverse-XOR
> `direct_peer_from_session_id` had no consumer → dropped; helpers introduced
> before their first caller → each lands with its consumer.
> **Old → new B-number remap** (re-pointed by meaning; old B4 splits): old B1·B2
> → B2; B3 → B1; B4 → B2 (mechanics) / B5 (lifecycle); B5 → B5; B6 → B3; B7 →
> B6; B8 → B4; B9 → B10; B10 → B7; B11 → B10; B12 → B8; B13 → B9.

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
  convention, so `ChatSessionKind` **must derive `Ord`** (confirming B2's "+
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
        // lifecycle (invited / joined): added by A4, enriched by A5.
        // voice-channel state (has-voice / joined-voice / membership): added A12.
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
- **history / unread (A8)**, the **lifecycle** (A4, enriched by A5) and the
  **voice-channel state (A12)** are deliberately **not** added by A2 — the
  struct grows additively as those items land (text *and* voice channel both
  hang off this one value), so each picks its own representation; no A2 rework.
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

### Session-lifecycle reference (from A4)

The state machine over the A2 `chat_sessions` registry: how each kind opens,
what "joined" means without a UDP ack, and what removes an entry. A4 adds one
field to `ChatSession` and wires the transitions into the *existing* outbound
methods and inbound handlers — no new command (A5 adds accept/decline). The
simulator stays authoritative; the lifecycle is an optimistic local mirror.

**The lifecycle field** (on `ChatSession`, the A2-deferred "invite status" slot,
now generalised). It tracks **session-level** membership — driven by the *text*
channel and our own actions; the **voice** channel's join-state is a separate
A12 facet on the same session. **A5 later enriches the `Invited` variant** to
carry the invitation payload (`Invited(PendingInvite { inviter, session_name,
channel })`); A4 fixes the two states and their transitions:

    enum ChatSessionLifecycle { Invited, Joined }   // A5: Invited(PendingInvite)

- **`Joined`** — we believe we are an active participant. This is the state for
  **every 1:1** (the moment it opens), a group/conference we **started**, one
  we **accepted** an invite to, and any session we have seen **inbound traffic**
  in. On the **UDP** path it is **optimistic** — no UDP "joined" ack,
  so `Joined` means "we acted / saw traffic", not "sim-confirmed". On the
  **modern CAPS** path A5 adds, the `ChatSessionRequest` `"accept invitation"`
  reply **does** confirm the join (and returns the roster — A5/A6), so a
  CAPS-accepted `Joined` is sim-confirmed. A4 keeps one `Joined` state for both;
  the optimism is a property of the UDP path, not of the state.
- **`Invited`** — a conference/group invite we have **not** acted on and have
  seen **no** traffic for. Set **only** by the A5 invitation path
  (`Event::ConferenceInvited`). A bare invite is the *one* non-`Joined` case.

1:1 never carries `Invited` (there is no IM invitation — you just message and it
opens). `chat_session_mut` (A2) creates with **`Joined`** by default (the common
"opened by our action / by traffic" case); A5's invite-create is the sole path
that overrides the new entry to `Invited` before any traffic.

**Open / join transitions** (each maps onto a real site; the inbound rows share
the handler A6 folds rosters into and A8 folds history into — B2 adds the
get-or-create + B5 the `lifecycle = Joined` stamp there):

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

### Invitation-handling reference (from A5)

How a chat-session invitation is tracked and accepted/declined. **Policy
(user-set): adopt the modern Second Life CAPS workflow wherever it exists, and
keep the UDP path only while even OpenSim still uses it.** For session invites
that means **both**: the modern `ChatSessionRequest` cap is the Second Life
path, and the UDP `ImprovedInstantMessage` path is the OpenSim path (OpenSim
**stubs** `ChatSessionRequest` — see below). The simulator stays authoritative;
the pending-invitations registry is a read model.

**Pending invitations = the A4 `Invited` entries** (no separate registry). A5
enriches A4's lifecycle enum so the `Invited` state carries the invite payload,
making the registry self-describing:

    enum ChatSessionLifecycle { Invited(PendingInvite), Joined }   // refines A4/B5

    struct PendingInvite {
        inviter: AgentKey,         // ConferenceInvited.from_agent_id
        session_name: String,      // ConferenceInvited.session_name
        channel: InviteChannel,    // which channel(s) we were invited to
    }

    enum InviteChannel { Text, Voice, Both }

- **`channel`** records whether the invitation is to the **text** channel, the
  **voice** channel, or both. The `ChatterBoxInvitation` body distinguishes them
  (Firestorm `llimview.cpp:5195`): an `instant_message` body is a *text* session
  invite (viewer auto-joins it), a `voice` body is a *voice-call* invite (the
  viewer prompts the user), an `immediate` body is an immediate IM. A group /
  conference can have **both** a text and a voice channel under one session id,
  so the two are tracked together, not as separate sessions.
- Fed by the existing `ChatterBoxInvitation` handler
  (`handle_caps_event`, `methods.rs:663` → `Event::ConferenceInvited`) and the
  UDP `SessionGroupStart` / `SessionConferenceStart` IM path: on an invitation,
  get-or-create the registry entry keyed by `from_group ? Group { group_id } :
  Conference { id }` and set `lifecycle = Invited(PendingInvite{…})`. The event
  is still emitted unchanged (the driver shows the invite and decides).
- The `Invited` payload is dropped when the entry promotes to `Joined` (accept,
  or any inbound traffic — the A4 promotion rule). So pending invitations are
  exactly `chat_sessions` entries whose `lifecycle` is `Invited(..)`, shown by
  the A10 `chat_sessions()` accessor — no second map.
- Only **group / conference** session invites exist (1:1 has none — you just
  message). `GroupInvitation` (dialog 3, a *join-the-group* offer) is a
  different feature and **out of scope** here.

**The two commands** (`command.rs`):

    AcceptChatInvite  { session_id: ImSessionId, from_group: bool }
    DeclineChatInvite { session_id: ImSessionId, from_group: bool }

`session_id` + `from_group` mirror the `ConferenceInvited` fields the driver is
answering (typed `ImSessionId` — a group session id is still an IM session id;
the `Group` key reinterprets it via `GroupKey::from(session_id.uuid())`).
The flat `session_id.uuid()` is exactly the `"session-id"` the CAPS body needs.

**Text vs voice methods on the shared cap (the distinction that matters).** The
one `ChatSessionRequest` cap carries *both* text-session and voice methods; A5
uses the **text** methods for the text channel and the **voice** methods for the
voice channel, never mixing them (Firestorm `llimview.cpp`):

| Action | Channel | `method` | Notes |
|--------|---------|----------|-------|
| join | text | `"accept invitation"` | reply body **is the participant roster** → seeds A6 (`:666`, `:721`) |
| leave/refuse | text | `"decline invitation"` | multi-agent decline (`:3437`) |
| join | voice | `"accept invitation"` **+ start voice channel** | same method, then the voice signalling join (A12 / the existing voice feature) (`:730`) |
| refuse | voice (multi-agent) | `"decline invitation"` | (`:3437`) |
| refuse | voice (1:1 / P2P) | `"decline p2p voice"` | P2P-only (`:3422`) |

The **`"accept invitation"` reply carries the session's current agent roster** —
A5 hands it to A6 as the initial participant list (the modern equivalent of the
UDP `SessionAdd` stream). A *voice* accept uses the **same** `"accept
invitation"` and then triggers the voice-channel join *signalling* (A12); the
actual audio is the external client (out of scope). A viewer **auto-accepts
text** invites and **prompts** for voice — sl-client surfaces both as the
`Invited` entry and leaves the accept/decline decision to the driver.

**Path selection lives in the runtime** (it owns the capability map and all CAPS
HTTP — the sans-IO `Session` cannot POST; mirrors `RequestOfflineMessages`):

- **`ChatSessionRequest` cap present (Second Life)** → POST
  `application/llsd+xml` `{ "method": <per table>, "session-id": <uuid> }` to
  the cap url, following the existing `post_voice_cap` / `post_caps_oneway`
  pattern (`sl-client-tokio` `http.rs`, `voice.rs`). A new constant
  `CAP_CHAT_SESSION_REQUEST = "ChatSessionRequest"`.
- **cap absent (OpenSim)** → **UDP fallback** (text channel): *accept* needs
  **no** wire — the sim added us when it routed the invite, so accepting is just
  the optimistic local `Invited`→`Joined`; *decline* sends a `SessionLeave`
  (`ImprovedInstantMessage`, the existing `leave_*`). OpenSim **voice** runs
  through its own FreeSwitch/Vivox modules, not `ChatSessionRequest`, so a voice
  invite is not exercised on the local grid.

**The sans-IO `Session` effect (always, regardless of path).** The registry
transition is pure state and lives in `Session`:

- `Session::accept_chat_invite(session_id, from_group, now)` → promote the entry
  to `Joined` (get-or-create as `Joined` if somehow absent).
- `Session::decline_chat_invite(session_id, from_group, now)` → **remove** the
  entry.

The runtime calls the `Session` method (registry) **and** does the transport:
the CAPS POST when the cap is present, otherwise the UDP `SessionLeave` for a
decline (accept has no UDP wire). So the registry stays correct on every grid;
only the *wire* differs by path. No new `Event` — accept/decline is a local
action the driver took; the session's joined-ness is later confirmed by inbound
traffic (A4's optimistic model).

**OpenSim test limitation (grounded).** `ChatSessionRequest` is **not**
implemented in OpenSim — both the FreeSwitch and Vivox voice modules have the
`caps.RegisterHandler("ChatSessionRequest", …)` line **commented out**, and the
stub handler just returns `<llsd>true</llsd>`
(`FreeSwitchVoiceModule.cs:296`, `VivoxVoiceModule.cs:434`). opensim-core has no
implementation at all. So the **modern accept/decline is Second-Life-only
testable** (live-aditi); the **UDP-fallback** accept/decline is what the local
OpenSim grid exercises. The implementation must therefore keep both paths real,
not treat UDP as a dead fallback.

### Participant & typing reference (from A6)

The two per-session collections on `ChatSession` (A2): the **roster** (who is in
the session) and the **typing set** (who is currently typing). Both are folded
from the existing inbound events with **no** event-surface change, and exposed
through accessors. The simulator stays authoritative; these are a read model.

**Roster — `participants: BTreeSet<AgentKey>`** (A2's field, type unchanged).

- **Folded** from `Event::GroupSessionParticipant` (`methods.rs:2016`) and
  `Event::ConferenceSessionParticipant` (`:2033`): `joined == true` →
  `insert(agent_id)`, `false` → `remove(agent_id)`. The fold goes through
  `chat_session_mut(kind, now)`, so a participant event **also opens**
  the session (the A4 rule — participant traffic is "joined" traffic); roster +
  lifecycle update at the one site (composing with B2 / B5).
- **Seeded** from the A5 modern path: the `ChatSessionRequest` `"accept
  invitation"` reply carries the session's current agent list (Firestorm
  `setSpeakers`), which B5 decodes straight into this set — the CAPS equivalent
  of replaying the `SessionAdd` stream.
- **1:1 is not materialised.** A `Direct` session's roster is implicitly
  `{ self, peer }`; `SessionAdd` / `SessionLeave` do not apply to it. The
  accessor synthesises `{ peer }` from the `Direct { peer }` key (self is
  `agent_id()`), so no storage is spent on 1:1 rosters.
- The set stores whatever the sim reports for group / conference, which
  **includes self** once we have joined (the sim lists us among the
  participants); the accessor returns it verbatim.

**Typing — `typing: BTreeMap<AgentKey, Instant>`** (A6 **refines** A2's
`BTreeSet<AgentKey>` to a map of *last-seen* times, to support auto-expiry).

- **Folded** from `Event::ImTyping` (`methods.rs:1995`): `typing == true` →
  insert / refresh `from_agent_id → now`; `typing == false` → remove
  `from_agent_id`.
- **Session resolution** (the wire `ImTyping` carries `session_id = block.id`
  and `from_agent_id`, but no `from_group`): if `session_id` matches a tracked
  `Group { id }` or `Conference { id }` entry → that session, typer =
  `from_agent_id`; **otherwise it is a 1:1** → key `Direct { peer:
  from_agent_id }` (the typer *is* the peer). Keying 1:1 off `from_agent_id`
  rather than reverse-XOR of `block.id` is deliberate: a 1:1 typing IM's `id`
  field is not reliably the `XOR` id across senders, but `from_agent_id` always
  identifies the peer.
- **Typing never opens a session** (unlike a message or a participant event). It
  uses a *non-creating* mutable lookup (a new `chat_session_get_mut(kind) ->
  Option<&mut ChatSession>`); if the session is not open, the `ImTyping` event
  still fires (the driver may react) but nothing is stored. Rationale: typing is
  ephemeral and unreliable; an empty session conjured by a stray "typing…" then
  cancelled would pollute the registry. Sessions still open on the first real
  message (A4).
- **Auto-expiry — yes.** A `TypingStop` can be lost (packet loss, a crashed
  peer), so a bare set would strand "X is typing…" forever. Each entry keeps its
  last-seen `Instant`; entries older than `TYPING_TIMEOUT` are pruned. The
  constant is **9 s** — Firestorm `OTHER_TYPING_TIMEOUT` (`fsfloaterim.cpp:88`);
  senders re-emit Start every ~4 s (`ME_TYPING_TIMEOUT`), so 9 s tolerates
  a couple of missed refreshes. Pruning runs in `poll(now)` (the session's
  existing timed loop), keeping the read accessor `now`-free; an explicit
  `TypingStop` still removes immediately.

**Outbound `send_im_typing` (`methods.rs:3835`) tracks nothing.** The typing set
holds **remote** typers only (for "who is typing *to* me"); our own outbound
typing is the driver's own action and is not mirrored into any session — so
`send_im_typing` is unchanged and adds no self entry.

**Accessors** (public; the maps stay private):

    fn participants(&self, session: ChatSessionKind)
        -> impl Iterator<Item = AgentKey> + '_   // group/conf: stored;
                                                 // Direct: synthesised { peer }
    fn typing(&self, session: ChatSessionKind)
        -> impl Iterator<Item = AgentKey> + '_   // live (non-expired) typers

**Interaction with A7.** A6 owns the storage; **A7** mutates it on
`FriendsOffline` — clearing an offlined friend's typing in every session and
dropping them from rosters where they appear. The auto-expiry above is an
independent backstop (a vanished typer clears after 9 s regardless); the two
**layer**, neither replaces the other. **Persistence:** rosters / typing live on
`ChatSession`, so they persist across teleport (grid-level, A9) and clear on
logout; typing additionally self-prunes.

### Presence-driven reset reference (from A7)

How friend presence (A3) drives chat-session state. When a friend goes offline
the chat state tied to them is cleaned **immediately**, rather than waiting on
the simulator's session events. This is the one place the two subsystems —
presence (A3) and the session registry (A2/A6) — couple; everywhere else they
are independent (A3's invariant). The simulator stays authoritative; A7 is a
fast, best-effort mirror that *layers with*, never replaces, the sim's
`SessionLeave`.

**Trigger: `OfflineNotification` → `FriendsOffline`** (`methods.rs:3514`). A3
already removes each offlined `FriendKey` from `online` here; A7 adds, at the
**same** handler, for each offlined agent `a` (`FriendKey` → `AgentKey`, same
underlying `Key`):

- **Clear their typing everywhere** — for every `ChatSession` in the registry,
  `typing.remove(a)`. A friend who logged out cannot still be typing; do it now
  rather than wait the A6 9 s expiry (which remains the backstop for non-friends
  and crashes).
- **Drop them from group / conference rosters** — for every `ChatSession`,
  `participants.remove(a)`. Logout removes the agent from every IM session, so
  this is correct; the sim will *also* send `SessionLeave` (A6 removes them
  again — idempotent), but A7 is faster and still cleans up if a crash means no
  `SessionLeave` arrives. A `Direct` session has no materialised `participants`,
  so this is a no-op there.

That is the whole fan-out: one pass over `chat_sessions`, removing `a` from each
session's `typing` and `participants`. Cost is O(sessions) per offlined friend —
trivial.

**What A7 does *not* do (the refinements):**

- **No session is removed.** A 1:1 is never removed (A4) — its history must
  survive the peer going offline; group / conference sessions we are in are not
  removed either (only an explicit leave / decline removes — A4). A7 only edits
  the *contents* (`typing` / `participants`), never the registry membership.
- **No per-session "offline" marker.** The sketch said "mark/close the 1:1"; the
  decision is **neither**. A 1:1's peer-offline state is exactly
  `!is_online(peer)`, already kept by A3's `online` set — the single source
  of truth. Storing a marker on the `ChatSession` would duplicate it and risk
  drift. The driver reads presence via `is_online(peer)` (A3) for any session it
  displays.
- **No lifecycle change.** A 1:1 stays `Joined` when its peer goes offline — you
  can still send (it becomes a stored offline IM); "joined the conversation" is
  unrelated to "peer currently online".

**`FriendsOnline` → no chat action.** Because no marker is stored,
there is nothing to clear when a friend comes back: A3 adds them to `online`
(which flips `is_online`), and that is the entire effect. The friend re-appears
in a roster only when the sim re-adds them (`SessionAdd`) or speaks — A7 does
**not** speculatively re-populate rosters. This keeps presence the only driver
the online set and avoids inventing membership.

**The friends-only caveat & layering (explicit).** Presence is **friends-only,
`CAN_SEE_ONLINE`-gated** (A3). So A7's roster/typing cleanup fires **only** for
participants who are our friends *and* grant us see-online. Every other
participant — non-friends, or friends not granting see-online — is cleaned up
**only** by the sim `SessionLeave` (A6 roster fold) and the A6 typing expiry.
The two signals **layer**: A7 is the fast path where presence is visible to us;
`SessionLeave` / expiry is the universal path. Neither replaces the other, and
both are idempotent (removing an already-absent key is a no-op), so a friend who
triggers both just gets removed once.

**Persistence.** A7 is triggered by *presence*, not by region change, so it is
orthogonal to the A9 teleport-persistence rules: presence (and thus the chat
state) survives a teleport because no `FriendsOffline` is synthesised by moving.
A7 fires only on a genuine `OfflineNotification`.

### History & unread reference (from A8)

The per-session conversation log + the unread marker. **Offline-IM retrieval is
already implemented** (A1 — `Command::RetrieveInstantMessages` UDP +
`Command::RequestOfflineMessages` CAPS), so A8 designs only the **in-memory**
bounded log + unread and how replayed offline IMs drain into it. *Long-term*
persistence to local files (read & write, all text-chat types, Firestorm-style)
is the separate **A13** item; this in-memory model is its working set and can be
seeded from A13's file read-back.

**The log entry.** A public value type (**renamed `SessionMessage` by A13**
— the sketch's `ChatMessage` collides with the existing nearby-chat
`ChatMessage`, `types/chat.rs:254`; the name `SessionMessage` threads through
B4 / A10 / B9):

    struct SessionMessage {        // A13 rename (was ChatMessage)
        sender: AgentKey,          // self for our own outbound sends
        dialog: ImDialog,          // Message (1:1) or SessionSend (group/conf)
        text: String,
        timestamp: Option<u32>,    // the wire Unix time (InstantMessage.timestamp)
    }

- **`timestamp`** is the wire `InstantMessage.timestamp` (Unix seconds, the sim
  fills it; the *original* time for an offline IM). It is `None` for our own
  outbound sends — the sans-IO `Session` has no wall-clock (`SystemTime` is
  banned), so the driver renders `None` as "now". Message **order** is the
  `VecDeque` insertion order, not the timestamp.
- `dialog` is kept for fidelity (per the A8 brief: sender / timestamp / text /
  dialog); in practice it is `Message` or `SessionSend`.

**The fields on `ChatSession`** (the A2-deferred history/unread slot):

    history: VecDeque<ChatMessage>,   // capped at HISTORY_CAP
    unread: u32,

- **`history`** is bounded by `HISTORY_CAP = 256`; on overflow the oldest
  is `pop_front`-ed. In-memory only (A9); A13 adds the optional file spillover
  long-term scrollback.
- **`unread`** is a plain count (not a shifting index, which a capped `VecDeque`
  would invalidate). On a prune that drops an unread message, clamp
  `unread = min(unread, history.len())` — a negligible edge at cap 256.

**What is logged** — *conversation only*:

- **inbound** 1:1 `Message` (the catch-all arm; this is also where the `Direct`
  session opens — A4/B2), and group / conference `SessionSend`
  (`methods.rs:2006` / `:2025`);
- **our own outbound** `send_instant_message` / `send_group_message` /
  `send_conference_message` (`sender = self`), so the log is a full transcript;
- **offline** replays (`offline == true`) ride the inbound `Message` path.

**Not** logged: typing, participant add/leave, inventory/offer/notice dialogs,
friendship, and `FromTask` (object→agent IM — it belongs to no tracked session).

**Unread.** Incremented by **one per inbound conversational message from another
agent** (offline IMs included — they are unseen). Our own outbound message
**resets** `unread` to 0 (replying implies you read it), and a new
`mark_session_read` / `Command::MarkSessionRead { session }` resets it too
(the driver calls it when the user views the session). Typing, participant and
system dialogs never touch `unread`.

**Offline drain (login).** The already-shipped retrieval replays stored IMs as
ordinary `Event::InstantMessageReceived` with `offline == true`. They flow
through the **same** inbound logging path: each opens / finds its
`Direct { from_agent_id }` session (A4), appends a `ChatMessage` carrying its
**original** wire `timestamp`, and bumps `unread`. So login populates the right
sessions with the right times — no offline-specific routing. *When*
retrieval fires is driver/runtime policy (the command exists; viewers
auto-request at login); A8 only routes the result.

**Accessors** (public; the deque stays private):

    fn history(&self, session: ChatSessionKind)
        -> impl Iterator<Item = &ChatMessage> + '_
    fn unread(&self, session: ChatSessionKind) -> u32
    fn total_unread(&self) -> u32          // sum across sessions, for a badge

**Persistence.** `history` / `unread` live on `ChatSession`, so they persist
across teleport (grid-level, A9) and clear on logout — *unless* A13's file
logging is enabled — the long-term store that outlives the session and is
read back on a later login.

### Persistence & region reference (from A9)

Where the chat/presence stores sit relative to the *region* lifecycle. The whole
system (`chat_sessions` A2, `friends` / `online` A3) is **grid-level** — routed
by the grid's IM / group / presence services, not the region simulator — so it
behaves as the **inverse** of the region-local `SitState` and the
per-in-world-object script-permission grants: those reset at every region
boundary, the chat/presence stores never do. A9 produces **no new state and no
new code path** — it *locks* a behaviour by fixing where the B2/B1 stores are
(and are not) wired, and pins the verification.

**The four region-boundary reset sites (where chat/presence must NOT appear).**
Each already resets the region-local state; the chat stores are absent from all
four and must stay absent:

| Site | What it resets today | Chat/presence |
|------|----------------------|---------------|
| `begin_handover` (`methods.rs:760`, retarget teleport) | `children` / `child_seeds` / `objects` / `terrain` / `regions` / `time_dilation` cleared; `sit = NotSitting` (`:800`); `drop_inworld_grants()` (`:803`) | **untouched** |
| `promote_child_to_root` (`:897`, neighbour crossing) | rebuilds the root circuit; **keeps** the seat (a vehicle carries the agent across — `:796`) | **untouched** |
| `TeleportLocal` (`:2237`, intra-region) | `sit = NotSitting` (`:2243`); `drop_inworld_grants()` (`:2244`) | **untouched** |
| `DisableSimulator` (`:1206`, child-circuit retire) | drops the child circuit / seed; `forget_sim_objects` | **untouched** |

The rule is therefore **"add no clear at those sites"**: when B2/B1 land the
stores, none of these four handlers gains a `chat_sessions` / `friends` /
`online` clear. There is no positive code — A9 is the guard that the grid-level
stores never get accidentally wired into the region-reset path (the easy mistake
is to "mirror" the `objects.clear()` line). The contrast is exact: `sit` and the
script grants are *region* facts (a seat is region-local; a grant is per
in-world object left behind), so they reset; a chat session / buddy presence is
a *grid* fact that the same teleport leaves wholly intact.

**Logout keeps them in memory — discard, never in-place clear** (revised
2026-06-27 on user request). Logout is terminal, not a reset: `close` (`:9599`),
`LogoutReply` (`:3548`), and the logout-timeout (`:3597`) each only set
`state = SessionState::Closed` (terminal — `is_closed`, `:9594`) and emit the
disconnect/`LoggedOut` event; **no field is cleared**. This is deliberate and
now load-bearing for the chat stores: a user logging out may still want to
**inspect the messages from immediately before logout**, so the chat sessions,
their history, the rosters, and the friend/presence stores **remain readable on
the `Closed` session** — the read accessors (`history` / `chat_sessions` /
`friends` / `is_online`) must **not** gate on `state` (they are pure getters, so
they already don't; B10 asserts it). The stores die only when the driver
**drops** the `Session`; a relogin constructs a **fresh** `Session::new(login)`
(`:151`) that starts empty. This is the A2/A3 "constructor rebuild, no `close`
hook" convention — **no logout-time clearing code, no reset hook** — and adding
one would now be a *regression*, destroying the post-logout history the user
wants (it mirrors how `sit` / `objects` / `script_grants` are *not* cleared on
close either — they too just vanish with the discarded struct).

**The constructor slot.** `Session::new` is a **`const fn`** (`:151`). The chat
fields go in beside `sit: SitState::NotSitting` (`:165`) and
`script_grants: BTreeMap::new()` (`:167`) as `chat_sessions: BTreeMap::new()` /
`friends: BTreeMap::new()` / `online: BTreeSet::new()` — all
const-constructible, so the constructor stays `const`. (B2/B1 add the fields;
A9 only fixes that they seed empty here and nowhere else.)

**Verification — the inverse of `teleport_clears_seat`.** The existing
`tests/lifecycle.rs:1716` `teleport_clears_seat` asserts the seat is **gone**
after a teleport; the A9 persistence test is its mirror image — seed a chat
session (+ history / roster) and a presence entry, drive a teleport / neighbour
crossing / `DisableSimulator`, and assert every chat/presence entry is **still
present** and unchanged. This is the single most load-bearing A9 check and is
listed in A11's strategy.

**Boundary with A13.** A9 governs the **in-memory** region behaviour only:
within one logged-in session the stores survive every region change and die with
the session. Persistence **across** logins (long-term scrollback) is **out** of
this in-memory model — it is A13's optional, default-off, runtime **on-disk**
chat-log file layer (the sans-IO `Session` does no I/O). A9 = in-memory /
region; A13 = on-disk / cross-session.

### API-surface & exposure reference (from A10)

The complete public delta the chat-session system adds — `Command`s, `Event`s,
`Session` accessors, the new view types — and how each of the three runtimes
surfaces it. A10 consolidates what B1–B6 produced into one coherent public API
and pins the **exposure model**, which is the load-bearing decision here (it
diverges, deliberately, from the strict PERMISSION "all reads via `Event`"
rule). The simulator stays authoritative; everything below is the read model +
the few outbound actions.

**Why the exposure model is not uniform (the architecture fact).** The sans-IO
`Session` exposes plain public read accessors, and **whoever holds the `Session`
calls them directly, zero-copy** — that is the real API for embedded users and
for tests. The two channel-based runtimes differ only because of *who owns the
`Session` at runtime*:

- **bevy** keeps the `Session` boxed inside a Resource (`SlDriver`), so a system
  can take a real `&Session` borrow and read accessors **directly, zero-copy,
  no `Arc`, no query round-trip**. This is the cheapest path and the one that
  matters for the user's large histories.
- **tokio** runs `Client::run(mut self, …)` (`sl-client-tokio/src/lib.rs:269`),
  which **consumes** the `Client` and is moved into a spawned task
  (`tokio::spawn(client.run(…))`, `sl-repl-tokio/src/bin/sl-repl-tokio.rs:587`).
  After that the app holds only the command-sender / event-receiver ends — there
  is no `&Session` to call. So tokio (and the **REPL**, which rides the tokio
  `Client`) read state back over the **pull bridge**: a query `Command` whose
  handler calls the accessor and synthesises a reply `Event` (the
  `QueryScriptPermissions` → `ScriptPermissionState`, `methods.rs:5739`
  / tokio dispatch `:1191` / bevy dispatch `:1963`).

**Parity is therefore redefined for the read path.** "Feature parity" across
the runtimes means **identical data, identical `Command`s, identical view
types** — *not* an identical read mechanism. bevy borrows; tokio/REPL pull. The
**outbound action** commands (below) stay byte-for-byte parity (the established
six-site pattern). This is the one place the roadmap relaxes the
PERMISSION-era "every read is an `Event`" rule, and it is a conscious trade for
zero-copy reads on bevy.

**The history-scale design (the user's concern: `chat.txt` = 3.8M lines,
largest IM 120k).** Two stores, and bulk history is **never wholesale-copied**:

- The **in-memory** store on `Session` is only the A8 **256-cap hot tail** per
  session (`HISTORY_CAP`). Small, and the only chat history the sans-IO core
  ever holds.
- The **deep archive** is **A13's on-disk** file layer (years of lines). It is
  read **on demand, one page at a time** — a cursor + `limit`, the file accessed
  by `seek` / `mmap`, parsing only the requested window. Only a *screenful*
  crosses any boundary, so per-read cost is independent of total archive size
  (standard infinite-scroll). A13 implements the disk side; A10 only fixes the
  cursor / page API so it can plug in zero-copy.
- The bounded tail is handed across tokio's channel as **`Arc<[ChatMessage]>`**
  (an `Arc` clone is O(1) — no deep copy), and pages likewise. bevy skips the
  `Arc` entirely and borrows the slice. So the only copies that ever happen are
  bounded windows, and on bevy not even those.

**New read surface — query `Command` → reply `Event`** (tokio/REPL pull path;
bevy calls the same builder accessors directly instead):

| Query `Command` | Reply `Event` | Payload |
|-----------------|---------------|---------|
| `QueryChatSessions` | `ChatSessions(Arc<[ChatSessionInfo]>)` | the **light** session list — no history |
| `QueryChatHistoryPage { session, before: Option<MessageCursor>, limit }` | `ChatHistoryPage { session, messages: Arc<[ChatMessage]>, prev: Option<MessageCursor> }` | one bounded page, newest-first; `prev` pages older |
| `QueryFriends` | `FriendsSnapshot(Arc<[FriendPresence]>)` | buddy cache + online flag |

(The page payload `ChatMessage` is **renamed `SessionMessage`** by A13 — it
collided with the existing nearby-chat `ChatMessage`, `types/chat.rs:254`; read
`Arc<[SessionMessage]>` throughout B7/B9.)

**New public view types** (`Arc`-friendly, `Clone + Debug`; the registry /
maps stay private):

    struct ChatSessionInfo {
        kind: ChatSessionKind,            // the typed id (B2)
        lifecycle: ChatLifecycleView,     // Joined | Invited{…} (flattened)
        participants: Vec<AgentKey>,      // group/conf roster; Direct → {peer}
        typing: Vec<AgentKey>,            // live (non-expired) typers (A6)
        unread: u32,                      // A8 marker
        // A12 appends voice fields: has_voice / joined_voice / voice members
    }

    enum ChatLifecycleView {              // flattens ChatSessionLifecycle (A4/A5)
        Joined,
        Invited { inviter: AgentKey, session_name: String,
                 channel: InviteChannel },
    }

    struct FriendPresence { friend: Friend, online: bool }   // Friend is Copy

    struct MessageCursor(/* opaque */);   // page token; A13 defines its innards

- `ChatSessionInfo` deliberately **omits `history` and `last_activity`** — the
  list stays light (history is the separate paged query; `last_activity` is an
  `Instant`, meaningless across the boundary, used only to **order** the list
  newest-first before it ships). `ChatMessage` is already public (B4).
- `MessageCursor` is **opaque**: it round-trips a page request without
  the app interpreting it. A13 picks its representation (an in-memory sequence
  number near head, a file byte-offset / line index deeper in), so the cursor
  can span the memory→disk boundary transparently.

**New snapshot-builder accessors on `Session`** (sans-IO; the bevy-direct and
the tokio-pull paths both call these — mirroring `script_permission_state`):

    fn chat_sessions_info(&self) -> impl Iterator<Item = ChatSessionInfo> + '_
    fn friends_presence(&self)   -> impl Iterator<Item = FriendPresence> + '_
    fn history_page(&self, session: ChatSessionKind,
                    before: Option<MessageCursor>, limit: usize)
        -> (&[ChatMessage], Option<MessageCursor>)   // in-memory tail only

`history_page` serves the in-memory tail and returns a `prev` cursor pointing
**into the archive** when the window reaches the oldest in-memory message; the
runtime/A13 continues older pages from the file. These compose the lower-level
B2/B1/B3/B4 accessors (`chat_sessions` / `history` / `unread` / `participants` /
`typing` / `friends` / `is_online`), which stay as the primitive read API.

**Outbound action surface (recorded here; owned by the producing items).** These
wire through all three runtimes at full parity (the six-site pattern: `Command`
variant → `Session` method → tokio match → bevy match → REPL registry → REPL
`format.rs` `event_name`):

| `Command` | Producing item | Status |
|-----------|----------------|--------|
| `AcceptChatInvite { session_id, from_group }` | B5 | new |
| `DeclineChatInvite { session_id, from_group }` | B5 | new |
| `MarkSessionRead { session }` | B4 | new |
| `AcceptFriendship { …, friend_id: FriendKey }` | B1 | **changed** (added field) |
| `InstantMessage` / `ImTyping` / `StartGroupSession` / `SendGroupMessage` / `LeaveGroupSession` / `StartConference` / `SendConferenceMessage` / `LeaveConference` / `RetrieveInstantMessages` / `RequestOfflineMessages` | A1 inventory | unchanged |

**No `Event` is removed**, and **no new push notification event is added**: the
existing inbound events (`InstantMessageReceived` / `ImTyping` /
`GroupSessionMessage` / `GroupSessionParticipant` / `ConferenceSessionMessage` /
`ConferenceSessionParticipant` / `ConferenceInvited` / `FriendList` /
`FriendsOnline` / `FriendsOffline` / `FriendRightsChanged`) **double as
change-notifications** — on any of them the read model has updated, so a tokio
app re-pulls and a bevy app simply reads next frame. Our own outbound sends /
`MarkSessionRead` mutate the model without an event, but the app initiated them,
so it already knows.

**The boundary — sl-proto `Session` vs application policy.** `Session`
owns: the in-memory read model (registry, presence, the 256-cap tail), the
optimistic lifecycle (A4), wire encode/decode, and the snapshot-builders. The
**app / runtime owns policy**: *when* to fire offline-IM retrieval (viewers
auto-request at login); *whether* to auto-accept a text invite vs prompt for a
voice one (A5); *when* to call `MarkSessionRead` (the user viewed the tab); the
**CAPS-vs-UDP path selection** for accept/decline (the runtime owns the
capability map + all CAPS HTTP — B5); and the **entire A13 file layer** (write,
paged read-back, `mmap`, serving `QueryChatHistoryPage` and bevy's older-page
reads). `Session` never decides policy and never does I/O.

**Deferred deltas folded at the Phase B consolidation.** **A12** (voice) appends
the voice fields to `ChatSessionInfo` (has-voice / joined-voice / voice
membership / channel info), the join/leave-voice `Command`s, and the voice
accessors. **A13** (chat-log files) appends the runtime file config and
implements the deep-history paging behind `QueryChatHistoryPage` (and bevy's
older-page reads) — no new sans-IO `Command`, since it is pure runtime I/O. B7
ships the A1–A9 read-out; the consolidation merges A12/A13's surface into it.

### Test & verification reference (from A11)

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

### Per-session voice-channel reference (from A12)

The per-`ChatSession` **voice** facet, at the SL **signalling** level only. A
group / conference / 1:1 session can carry a voice channel beside its text
channel (2026-06-27 scope expansion); A12 tracks *that the session has voice,
the channel coordinates, whether we have joined, and who is in it* — **never**
the audio stream nor who is currently speaking. It **reuses** the existing
voice-signalling feature wholesale and adds only per-session state on top.

**What already exists vs. what A12 adds.** Three voice surfaces, kept separate:

| Surface | Scope | Existing? | Owner |
|---------|-------|-----------|-------|
| Voice **account** (`VoiceAccountInfo`, `RequestVoiceAccount` → `VoiceAccountProvisioned`, `methods.rs:495`) | agent-global credentials to the voice *server* (Vivox SIP / WebRTC JSEP) | yes (`sl-wire voice.rs:329`) | unchanged |
| **Parcel / spatial** channel (`ParcelVoiceInfo`, `RequestParcelVoiceInfo` → `ParcelVoiceInfo`, `methods.rs:500`) | nearby / local voice (**not** a chat session) | yes (`sl-wire voice.rs:494`) | unchanged |
| **Per-session** channel (`VoiceChannelState` on `ChatSession`) | the group / conference / 1:1 *room's* voice | **no — A12 adds it** | A12 / B8 |

The account is provisioned once per login; the per-session join *uses* that
account to connect to a session's channel. A12 does **not** re-provision
— it records the per-session room state and triggers the existing plumbing.

**The state (additive on `ChatSession`, the A2/A5-reserved slot):**

    struct VoiceChannelState {
        has_voice: bool,                   // session offers a voice channel
        channel: Option<VoiceChannelInfo>, // coordinates (uri / creds)
        joined: bool,                      // we joined at the SIGNALLING level
        members: BTreeSet<AgentKey>,       // who is in voice, not speaking
    }

    struct VoiceChannelInfo {
        channel_uri: Option<url::Url>,         // sip:… / the session voice room
        channel_credentials: Option<String>,  // per-channel credentials
        voice_server_type: Option<String>,    // "vivox" | "webrtc"
        session_handle: Option<String>,        // the SL voice session handle
    }

- `VoiceChannelInfo` mirrors the SL session `voice_channel_info` block and the
  existing `ParcelVoiceInfo`'s nested `voice_credentials` (`channel_uri` /
  `channel_credentials`) — a small **client-local** struct in sl-proto, not a
  reuse of `ParcelVoiceInfo` (whose `parcel_local_id` / `region_name` are
  parcel-only). `Default`-able (all `Option` / `false` / empty set), so
  `ChatSession::new` initialises an empty, no-voice facet.
- `members` is the **voice-connected** subset of the text roster (A6) — strictly
  a membership set, **never** the talk-activity / speaking state.

**Where each field is fed:**

| Field | Source | Decoder |
|-------|--------|---------|
| `has_voice` / `channel` | the A5 voice **invite body** (`ChatterBoxInvitation` `voice` body) and the `ChatSessionRequest "accept invitation"` reply `voice_channel_info` | **gaps:** B5 classifies `InviteChannel`; **B8 decodes the channel** (the invitation decoder ignores `voice` today — `conversions.rs:2521`) |
| `joined` | **optimistic**: set by the A5 voice-accept or a new `JoinSessionVoice`; cleared by `LeaveSessionVoice` / a voice decline | sans-IO state only (no audio ack) |
| `members` | the modern `ChatterBoxSessionAgentListUpdates` agent-list **voice** flag (the voice-connected subset) | **gap:** B8 decodes the agent-list voice flag — **not** the `is_now_speaking` flag (out of scope) |

**New commands (signalling-level; full six-site parity):**

    JoinSessionVoice  { session: ChatSessionKind }
    LeaveSessionVoice { session: ChatSessionKind }

- The **sans-IO `Session`** only records the per-session `voice.joined`
  transition (optimistic, like A4's text `Joined`) and exposes the accessors.
- The **runtime** orchestrates the existing voice plumbing: on join, ensure a
  voice account (`RequestVoiceAccount`, once) then signal into the session's
  channel via `ChatSessionRequest` (the same cap A5 uses, with the *voice*
  methods); on leave, `"decline invitation"` (multi-agent) / `"decline p2p
  voice"` (1:1 P2P) per A5, or the WebRTC teardown
  (`RequestVoiceAccount{logout}` / `SendVoiceSignaling{completed}`). No new HTTP
  helper — `post_voice_cap` / `post_chat_session_request` (B5) cover it.

**Accessors** (read model; fold into the A10 `ChatSessionInfo` view):

    fn session_has_voice(&self, session: ChatSessionKind) -> bool
    fn session_voice_channel(&self, session: ChatSessionKind)
        -> Option<&VoiceChannelInfo>
    fn session_voice_joined(&self, session: ChatSessionKind) -> bool
    fn session_voice_members(&self, session: ChatSessionKind)
        -> impl Iterator<Item = AgentKey> + '_

`ChatSessionInfo` (A10) gains the reserved voice fields `has_voice: bool`,
`voice_joined: bool`, `voice_members: Vec<AgentKey>` (and the channel info if a
driver wants it), so a UI can show a voice indicator + roster without a separate
query. This is the A10-noted "A12 appends voice fields to `ChatSessionInfo`".

**All three kinds carry voice.** Group / conference are multi-agent voice; a 1:1
is a **P2P voice call** (A5's `"decline p2p voice"`), so a `Direct` session's
`VoiceChannelState` is valid, its `members` implicitly `{ self, peer }`. This
**closes the A11 open question** "the voice-channel cases of A12".

**Boundary (user-set, restated).** sl-client models voice **session state** —
has-voice, channel coordinates, joined-at-signalling, membership — and **nothing
audio**: Vivox / WebRTC media transport and the "who-is-currently-speaking" /
talk-activity indicators live in **external voice client**. The crate's voice
feature is signalling only (the standing project rule).

**Persistence.** `VoiceChannelState` lives on `ChatSession`, so it follows
the A9 rule: it **persists** across teleport / crossing and clears only
on logout (with the rest of the session). A7's reset also drops an
offlined friend from `voice.members` (the same fan-out as `participants` /
`typing`), idempotent with the agent-list updates.

**OpenSim limitation.** `ChatSessionRequest` is stubbed in both OpenSim voice
modules and voice runs through its own FreeSwitch / Vivox path, so the
per-session voice flow is **SL-only testable** (live-aditi) — the identical
constraint A5 noted for the modern accept/decline. The local OpenSim grid
exercises only the text channel.

### Local chat-log files reference (from A13)

The optional, default-off **runtime** chat-log file feature: write every
text-chat line to a per-conversation transcript and read it back for long-term
scrollback. It lives entirely in the runtimes (`sl-client-tokio` /
`sl-client-bevy` / the REPL) — the sans-IO `Session` does **no** file I/O — and
is fed by the **event stream** plus our own outbound commands. Grounded in
Firestorm `LLLogChat` (`lllogchat.cpp`) and `LLConversationLog`
(`llconversationlog.cpp`), and **format-compatible** with them so the files
interleave with a Firestorm install.

**What is logged, and to which file.** All four text-chat types, each from its
event (and our matching outbound command for our own lines):

| Type | Event | File name |
|------|-------|-----------|
| nearby / local | `Event::ChatReceived` (`event.rs:374`) | `chat.txt` |
| 1:1 IM | `Event::InstantMessageReceived` (dialog `Message`) | `<account>.txt` (legacy `firstname.lastname.txt` option) |
| group | `Event::GroupSessionMessage` | `<group name> (group).txt` |
| conference | `Event::ConferenceSessionMessage` | `Ad-hoc Conference hash<md5-of-sorted-participant-ids>.txt` |

- Nearby chat is **out of the session-state scope** (no `ChatSession` is opened
  for it — A1) but **in** scope for logging; A13 is the *only* place nearby chat
  is persisted.
- Names are sanitised with Firestorm `cleanFileName` (every char in
  ``"'\/?*:.<>|[]{}~`` → `_`). Files live in a per-account `chat_logs/`
  directory. Optional **date suffix** (`LogFileNamewithDate`): `-%Y-%m-%d` for
  nearby, `-%Y-%m` (monthly) for IM / group; **never** for ad-hoc.
- The per-account directory + the 1:1 / group names come from the runtime's
  `login_account` (`methods.rs:6959`) and the events' `from_name` /
  `GroupSessionMessage.group_id`; the conference hash is the MD5 of the sorted
  participant ids (A6 roster).

**The line format** (Firestorm `LLChatLogFormatter`, `lllogchat.cpp:1041`):

    [YYYY/MM/DD HH:MM:SS]  Name: message

- **Seconds are ON by default** (user-set, 2026-06-27) — Firestorm's
  `DATE_FORMAT_SEC = "%Y/%m/%d %H:%M:%S"`, which it gates behind the optional
  default-*off* `FSSecondsinChatTimestamps`; A13 flips that default to **on** so
  log lines carry `HH:MM:SS`. Byte-compatible: Firestorm's parser reads the
  seconds variant (`TIMESTAMP_AND_STUFF_SEC`, `:92`).
- **Two spaces** separate the `]` from the name (Firestorm `IM_SEPARATOR`
  context). Multi-line messages: each embedded newline is written as `\n␠` (a
  newline + one leading space), so continuation lines are space-prefixed and the
  parser re-joins them. A literal colon in a name is URI-encoded `%3A`. A system
  message with no sender writes the name `Second Life:`.
- Toggles (config below): timestamp on/off (`LogTimestamp`), the date component
  (`LogTimestampDate` → `[HH:MM:SS]` time-only for today), seconds
  (default on, may be turned off), 24-hour vs 12-hour AM/PM.

**Read-back — reconciled with A10's paging (the load-bearing correction).**
Firestorm reads the last `LOG_RECALL_SIZE = 20480` bytes on open and *seeds the
in-memory buffer*. A13 was originally written the same way ("seed the A8
in-memory `history`"), but **A10's `QueryChatHistoryPage` supersedes that**: the
file **is** the deep archive, so —

- the sans-IO `Session` ring keeps only **this-session live** messages (the A8
  256-cap tail); it is **not** seeded from the file, and there is **no** new
  "load history" command;
- **B9 serves the *older* pages of `QueryChatHistoryPage` from file** — when
  A10's `prev` cursor points past the in-memory tail, the runtime reads the file
  (`seek` to the window / `mmap`), parses the lines back into `SessionMessage`s,
  and returns the page. `LOG_RECALL_SIZE` (20 KB) is the default seek/page
  window;
- on a **fresh login** the ring is empty, so *all* scrollback for an opened
  conversation comes from the file via paging — which is exactly the Firestorm
  "recall on open" behaviour, expressed through A10's pull API.

A stored line is parsed into `(timestamp, name, message)`; a line that fails
the format regex is kept as a **plain-text** `SessionMessage` (Firestorm
fallback), and space-prefixed continuation lines fold into the prior message.

**Optional `conversation.log` index** (`llconversationlog.cpp`). A per-account
metadata index of past conversations, default off, format-compatible:

    [<unix>] <type> <reserved> <offline> <name>| <pid> <sid> <file>|

(`type` 0=P2P / 1=group / 2=adhoc; `reserved` always 0; `offline` = has-unread).
Used for conversation discovery without scanning transcripts; entries older than
a retention (Firestorm `FSConversationLogLifetime`, default 30 days)
are purged on load.

**Config — `ChatLogConfig` (runtime, default OFF).** Opt-in, mirroring
Firestorm's per-account toggles:

- **enable per text-chat type** — nearby / IM / group / conference independently
  (nearby **default off**, as Firestorm; the `KeepConversationLogTranscripts`
  tri-state is the precedent);
- **log dir** (default the per-account `chat_logs/`);
- **filename scheme** — modern `<account>` vs legacy `firstname.lastname`
  (`UseLegacyIMLogNames`), the date-suffix toggle;
- **timestamp format** — timestamp on/off, date on/off, **seconds on/off
  (default ON)**, 24h/12h;
- **recall window** (default `LOG_RECALL_SIZE` 20480 bytes) — the page size B9
  reads from the file;
- the optional `conversation.log` index on/off + its retention days.

**Wall-clock — the runtime supplies it.** The sans-IO core has no clock (A8's
`SessionMessage.timestamp` is `None` for our own sends, the wire `timestamp` for
inbound). A13 is runtime, so it stamps lines with `SystemTime::now()` in **local
time** — file lines get real dates even for our own sends. For an inbound
message it prefers wire `InstantMessage.timestamp` (Unix; the *original* time
for an offline IM that may be replayed long after it was sent) and falls back to
receipt-now.

**Naming correction (cross-cutting, owned here).** A8/B4's planned history-entry
type `ChatMessage` **collides** with the pre-existing nearby-chat `ChatMessage`
(`types/chat.rs:254`, a different struct: `from_name` / `source` / `chat_type` /
…). A13 renames the A8 entry **`SessionMessage`**; it threads through B4 (the
field type), A10 / B7 (`ChatHistoryPage { messages: Arc<[SessionMessage]> }`,
the snapshot builders) and B9 (the parse target). Cross-references added at the
A8 and A10 reference sections.

**Boundary & testability.** A8 is the in-memory working set (this session, the
256-cap ring); A13 is the long-term on-disk store (all history) and the **only**
file I/O in the system — it spills A8's messages to disk on arrival and serves
the archive back through A10's paging. Because it is purely local file I/O, A13
is **grid-agnostic and testable on any grid** (write a message, assert the file
line; read it back, assert the `SessionMessage`) — unlike the SL-only voice
paths (A5 / A12). **This closes A11's "local chat-log file cases" question.**

## Phase B tasks — consolidated (B1–B10)

The draft per-A-item tasks (originally B1–B13) were merged and reordered into
the ten dependency-ordered tasks below to eliminate dead code / rework
**between** tasks: with `sl-proto`'s `[lints.rust]` denying the `unused_*`
family and the ggh pre-commit re-running full clippy on every attempt, an
intermediate commit that adds a field nothing reads, or an enum variant nothing
constructs, fails the gate. So each task adds every field/type **with** its
writer (fold / method), its reader (accessor / test), and tests, leaving the
tree buildable, clippy-clean (restriction lints), and `cargo test`-green on its
own. The reference subsections above are unchanged design records; the
`(was old B#)` tag on each task maps it back to the draft it absorbs (see the
remap table in the consolidation note).

Work these top-to-bottom; tick a box only when the step builds, is clippy-clean,
and `cargo test` passes. Keep `sl-client-tokio`, `sl-client-bevy`, and the REPL
at feature parity; never push client-only types into shared `sl-types`.
**Ask the user before starting Phase B** (the standing "ask before new roadmap
work" rule).

### B1. Friend-presence cache (buddy list + online set) — DONE 2026-06-27

*(was old B3 — from A3.)* Fully standalone (no registry dependency); first
because it is clean on its own and B6 consumes it. See § Friend-presence
reference (from A3).

- [x] Add `friends: BTreeMap<FriendKey, Friend>` + `online: BTreeSet<FriendKey>`
  to `Session` (`session.rs`), const-empty in `Session::new`.
- [x] Seed `friends` at the `FriendList` site (`methods.rs:1078`) from the same
  `friend()`-mapped data; leave `online` empty at login.
- [x] Fold each existing handler (record in addition to emitting its event):
  `OnlineNotification` (`:3504`) inserts into `online`; `OfflineNotification`
  (`:3514`) removes; `ChangeUserRights` (`:3524`) updates the cached `Friend`'s
  rights by `granted_to_us` (ignore if absent); `TerminateFriendship` (`:2586`)
  removes from both stores.
- [x] Live friendship add (both directions): on `ImDialog::FriendshipAccepted`
  in the inbound IM dispatch, insert `from_agent_id` with default
  `CAN_SEE_ONLINE` both ways; add a `friend_id: FriendKey` field to
  `Command::AcceptFriendship` + a param to `accept_friendship`, inserting the
  friend on accept with the same default. Wire the changed command 6-site
  (tokio / bevy / REPL) at parity.
- [x] Accessors `friends()` / `friend(id)` / `is_online(id)` /
      `online_friends()` returning public `Friend` / `bool` / `FriendKey`.
- [x] Invariant: no IM / chat-session path mutates `online`; assert it (deliver
  an IM after an `OfflineNotification`; the peer stays offline). Unit tests for
  every handler above (seed, online/offline, rights by direction, unknown-friend
  ignored, terminate drops both, both live-add paths).

**Deviation (needs review):** the design assumed `FriendKey` was `Ord` (it is
the `BTreeMap`/`BTreeSet` key), but the sl-types key newtypes derive no `Ord`.
Rather than the local-`Ord`-wrapper workaround (`ScriptHolder` precedent —
impossible for a *bare* foreign key), added `PartialOrd, Ord` derives to `Key`
and `FriendKey` in the shared **sl-map-tools/sl-types** crate (purely additive).
This honours the roadmap-literal `BTreeMap<FriendKey, Friend>` and the
newtype-over-raw preference; B2 will need the same on `AgentKey` / `GroupKey` /
`ImSessionId` for `ChatSessionKind: derive(Ord)`.

### B2. Chat-session registry + open/track mechanics — DONE 2026-06-27

*(was old B1 + old B2 + the non-lifecycle half of old B4 — from A1·A2·A4.)* The
typed discriminator, the registry storage/keying, and the get-or-create / remove
mechanics that fill it from the message handlers — but **no** lifecycle state
yet (that is B5). See § Inventory & unified-model reference (from A1),
§ State-model & keying reference (from A2), and § Session-lifecycle reference
(from A4).

- [x] Add the `ChatSessionKind` discriminator —
      `Direct { peer: AgentKey }` / `Group { group_id: GroupKey }` /
      `Conference { id: ImSessionId }`
      (derives incl. `Ord` — it is the map key) in a new chat-session module
      (`session/chat_session.rs`), with the canonical-id helper
      (`ChatSessionKind::canonical_session_id`, **`pub`** so the round-trip is
      testable) reusing `compute_im_session_id` (`conversions.rs:808`). Did
      **not** add the reverse-XOR `direct_peer_from_session_id` helper — no
      consumer keys 1:1 off the XOR id (B3 keys typing by `from_agent_id`).
- [x] Add `ChatSession { last_activity: Instant }` (only this field) +
  `ChatSession::new(now)` (no `Default` — `Instant` has none; `const fn`). Added
  the private `chat_sessions: BTreeMap<ChatSessionKind, ChatSession>` to
  `Session`, const-empty in the `const fn` constructor beside `online`.
- [x] Added only the `chat_session_mut(kind, now)` get-or-create helper (the
  read-only `chat_session` / non-creating `chat_session_get_mut` land with their
  first consumer in B3).
- [x] Fold get-or-create into outbound `start_group_session` /
      `start_conference` / `send_group_message` / `send_conference_message` /
      `send_instant_message` (1:1 opens here) and inbound group/conf
      `SessionSend` + the participant arms + the new explicit 1:1 `Message` arm
      (only dialog 0 opens a Direct session — other non-session dialogs do not);
      `leave_group_session` / `leave_conference` **remove** the entry; 1:1 is
      never removed.
- [x] Public `chat_sessions() -> impl Iterator<Item = ChatSessionKind>` lister,
  ordered by `last_activity` **newest-first** (kind breaks ties; so the field is
  read).
- [x] **Persistence guard (from A9):** added **no** `chat_sessions` clear at the
  four region-boundary sites (`begin_handover`, `promote_child_to_root`,
  `TeleportLocal`, child `DisableSimulator`); the store is grid-level. (Tests
  live in B10.)
- [x] Unit tests (11 new, `tests/lifecycle.rs`): open per kind (outbound +
  inbound), a non-session dialog opens nothing, `leave_*` removes, 1:1 persists,
  creates-once + restamps `last_activity` (observed via the newest-first
  ordering), the canonical-id round-trip per kind.

### B3. Participant & typing tracking

*(was old B6 — from A6.)* Adds the roster + typing collections, filled at B2's
handler arms and read through accessors. See § Participant & typing reference
(from A6).

- [ ] Add `participants: BTreeSet<AgentKey>` + `typing: BTreeMap<AgentKey,
  Instant>` to `ChatSession`; add the `TYPING_TIMEOUT: Duration = 9s` constant;
  add the non-creating `chat_session_get_mut(kind)` and read-only
  `chat_session(kind)` helpers beside B2's `chat_session_mut`.
- [ ] Roster fold: in the `GroupSessionParticipant` (`:2016`) /
  `ConferenceSessionParticipant` (`:2033`) arms, insert/remove `agent_id` by
  `joined` via `chat_session_mut` (same get-or-create B2 uses — they compose).
- [ ] Typing fold: in the `ImTyping` arm (`:1995`), resolve the session (a
  tracked `Group`/`Conference` by `session_id`, else `Direct { from_agent_id }`)
  via `chat_session_get_mut` (no create); `from_agent_id → now` on `true`,
  remove on `false`.
- [ ] Expiry: in `poll(now)`, prune typing entries older than `TYPING_TIMEOUT`.
- [ ] Accessors `participants(session)` / `typing(session)` (Direct participants
  synthesised as `{ peer }`). `send_im_typing` unchanged (no self tracking).
- [ ] Tests: `SessionAdd`/`SessionLeave` insert/remove + open the session;
      `participants(Direct)` = `{peer}`; `ImTyping` start/stop; 1:1 typing keys
      by `from_agent_id`; typing does **not** open a session; entry expires
      after `poll` passes `TYPING_TIMEOUT`; `TypingStop` clears immediately.

### B4. Per-session history & unread

*(was old B8, with old-B13's `SessionMessage` rename applied up front — from
A8·A13.)* Uses the non-colliding name `SessionMessage` from the start (the
existing nearby-chat `ChatMessage` lives at `types/chat.rs:254`). See § History
& unread reference (from A8) and § Local chat-log files reference (from A13).

- [ ] Add the public `SessionMessage { sender: AgentKey, dialog: ImDialog, text:
  String, timestamp: Option<u32> }`; add `history: VecDeque<SessionMessage>` +
  `unread: u32` to `ChatSession`; `HISTORY_CAP = 256`.
- [ ] Inbound log: in the group/conf `SessionSend` arms (`:2006` / `:2025`) and
      the 1:1 `Message` arm (the same sites B2 opens / B5 promotes), push the
      `SessionMessage`, `unread += 1` (skip when `sender == self`), prune to
      `HISTORY_CAP`; offline (`offline == true`) IMs ride the inbound path with
      their wire `timestamp`.
- [ ] Outbound log: in `send_instant_message` / `send_group_message` /
  `send_conference_message`, append `{ sender: self, .. }` and set `unread = 0`.
- [ ] Add `mark_session_read(session)` + `Command::MarkSessionRead { session }`
  (tokio / bevy / REPL at parity) resetting `unread`.
- [ ] Accessors `history(session)` / `unread(session)` / `total_unread()`.
- [ ] Tests: inbound 1:1 logs `unread == 1`; group `SessionSend` logs to the
      group session; own outbound logs (`sender = self`) + resets;
      `mark_session_read` resets; `HISTORY_CAP + 1` drops oldest; an
      `offline == true` IM logs its wire `timestamp` + bumps `unread`;
      `total_unread` sums.

### B5. Invitation handling + session lifecycle

*(was old B5 + the lifecycle half of old B4 — from A4·A5.)* The lifecycle enum
is born **here**, with its only constructor (the invite path) and its reader, so
the `Invited` variant is never a dead, never-constructed variant. See § Session-
lifecycle reference (from A4) and § Invitation-handling reference (from A5).

- [ ] Add `enum ChatSessionLifecycle { Invited(PendingInvite), Joined }` +
  `PendingInvite { inviter: AgentKey, session_name: String, channel:
  InviteChannel }` + `enum InviteChannel { Text, Voice, Both }`; add the
  `lifecycle` field to `ChatSession` (`ChatSession::new` default `Joined`).
- [ ] Promotion rule: any inbound session message / participant traffic sets
  `lifecycle = Joined` (dropping the payload) at B2's handler arms.
- [ ] Classify the invite channel: extend `chatterbox_invitation_from_llsd`
  (`conversions.rs:2521`) to read the `voice`/`instant_message` body; in the
  `ChatterBoxInvitation` handler (`methods.rs:663`) and the UDP
  `SessionGroupStart` / `SessionConferenceStart` dispatch, get-or-create the
  `Group`/`Conference` entry as `Invited(PendingInvite { .. })`; keep emitting
  `Event::ConferenceInvited` unchanged.
- [ ] Add `Command::AcceptChatInvite` /
      `DeclineChatInvite { session_id, from_group }` +
      `Session::accept_chat_invite` / `decline_chat_invite` (accept → `Joined`;
      decline → remove). The CAPS accept-reply roster decodes into the B3
      participant list.
- [ ] Runtime dual path (tokio / bevy / REPL at parity): look up
  `CAP_CHAT_SESSION_REQUEST`; if present POST `{ method, session-id }` with the
  per-channel method (text accept/decline; voice = accept + the voice-join
  signalling deferred to B8 / `"decline p2p voice"`), via a new
  `post_chat_session_request` helper; if absent, text decline sends
  `SessionLeave`. Add the cap to the requested-caps set / `caps` plumbing.
- [ ] Surface `lifecycle` through the `chat_sessions()` lister (so the field is
  read before B7).
- [ ] Tests: text `ConferenceInvited` (group + conf) → `Invited(channel=Text)`;
      voice → `Voice`/`Both`; `accept_chat_invite` → `Joined`; inbound traffic
      also promotes; `decline_chat_invite` removes; per-channel CAPS
      `method`/`session-id` LLSD unit; accept-reply roster decodes to
      participants.

### B6. Presence-driven auto-reset

*(was old B7 — from A7.)* The sole coupling between presence (B1) and the
registry (B2/B3). See § Presence-driven reset reference (from A7).

- [ ] In the `OfflineNotification` handler (`methods.rs:3514`), after B1's
  `online.remove(friend)`, for each offlined agent iterate `chat_sessions` and
  `typing.remove(agent)` + `participants.remove(agent)` (convert `FriendKey` →
  `AgentKey` via the shared `Key`).
- [ ] `FriendsOnline` (`:3504`) gains **no** chat code. No session removal, no
  lifecycle change, no stored offline marker, no new `Event`.
- [ ] Tests: seed a conference roster + a 1:1 typing entry for a friend, deliver
      `OfflineNotification`, assert they are gone from roster + `typing` **but**
      the sessions still exist and `is_online` is false; a non-friend is
      untouched (only `SessionLeave` removes them); `FriendsOnline` re-adds,
      changes no session.

### B7. Chat read-model exposure + the query/page API

*(was old B10 — from A10.)* Consolidates the aggregate view + the divergent
runtime exposure (bevy direct borrow; tokio/REPL pull). See § API-surface &
exposure reference (from A10).

- [ ] Add the view types `ChatSessionInfo` / `ChatLifecycleView` /
      `FriendPresence` / opaque `MessageCursor` (`Clone + Debug`,
      `Arc`-friendly) and the snapshot-builders `chat_sessions_info()` /
      `friends_presence()` / `history_page(session, before, limit)` on
      `Session`, composing the B1–B5 primitives. The list builder orders
      newest-first by `last_activity` and omits history.
- [ ] Add query `Command`s `QueryChatSessions` / `QueryChatHistoryPage {
  session, before, limit }` / `QueryFriends` and the reply `Event`s
  `ChatSessions(Arc<[ChatSessionInfo]>)` / `ChatHistoryPage { session, messages:
  Arc<[SessionMessage]>, prev }` / `FriendsSnapshot(Arc<[FriendPresence]>)` —
  `Arc<[…]>` payloads, never deep `Vec` copies.
- [ ] tokio: handle the queries by calling the builder and `events.send` (the
  `QueryScriptPermissions` arm `lib.rs:1191` is the template); no wire send.
- [ ] bevy: expose the read model by direct `&Session` borrow from a system (the
  Session lives in the `SlDriver` Resource); also accept the same query commands
  for parity (`lib.rs:1963` template).
- [ ] REPL: register the three query commands (`registry.rs`); add the reply
  `event_name` arms (`format.rs`); print the list / page / friends snapshot.
- [ ] Tests: `QueryChatSessions` → light list newest-first, `Invited` flattened;
      `QueryChatHistoryPage` → bounded newest-first page + `prev`, paging walks
      older windows without materialising the whole history; `QueryFriends` →
      right `online` flag; replies are `Arc<[…]>` (clone the `Arc`, compare
      pointers); a bevy-direct read of `chat_sessions_info()` matches the tokio
      query reply.

### B8. Per-session voice-channel state (signalling only)

*(was old B12 — from A12.)* After B5 (the `ChatSessionRequest` cap + invite
classification) and B7 (`ChatSessionInfo`). Reuses the existing voice-signalling
feature; **no** audio transport / speaking indicators. See § Per-session voice-
channel reference (from A12).

- [ ] Add `VoiceChannelState { has_voice, channel: Option<VoiceChannelInfo>,
  joined, members: BTreeSet<AgentKey> }` + `VoiceChannelInfo { channel_uri,
  channel_credentials, voice_server_type, session_handle }` (client-local,
  `Default`-able) and a `voice: VoiceChannelState` field on `ChatSession`
  (empty / no-voice in `ChatSession::new`).
- [ ] Decode the gaps: extend `chatterbox_invitation_from_llsd` to read the
      `voice` body into `has_voice` / `channel` (shared with B5); decode the
      `ChatSessionRequest "accept invitation"` reply `voice_channel_info`;
      decode the `ChatterBoxSessionAgentListUpdates` agent-list **voice** flag
      into `members` (**not** the `is_now_speaking` flag).
- [ ] Add `Command::JoinSessionVoice` / `LeaveSessionVoice { session }` +
  `Session::join_session_voice` / `leave_session_voice` (optimistic
  `voice.joined`). Runtime orchestration at parity: join = ensure
  `RequestVoiceAccount` then the `ChatSessionRequest` voice request; leave =
  `"decline invitation"` / `"decline p2p voice"` or the WebRTC logout — reusing
  `post_voice_cap` / `post_chat_session_request` (B5).
- [ ] Accessors `session_has_voice` / `session_voice_channel` /
  `session_voice_joined` / `session_voice_members`; add the voice fields to
  `ChatSessionInfo` (B7) and populate them in `chat_sessions_info()`.
- [ ] Fold `voice.members` into B6's offline fan-out; voice state persists a
  teleport (B10), clears on logout.
- [ ] Tests (SL-only flow noted; unit-test decoders on canned LLSD): voice
  `ConferenceInvited` sets `has_voice` + `channel`; accept-reply populates
  `VoiceChannelInfo`; `Join`/`LeaveSessionVoice` flip `session_voice_joined`; an
  agent-list voice update folds `members` (speaking flag ignored); 1:1 P2P voice
  carries `{ self, peer }`; `OfflineNotification` drops a friend from
  `voice.members`; a teleport preserves the voice facet.

### B9. Optional local chat-log files (runtime, all text chat)

*(was old B13, minus the `SessionMessage` rename already done in B4 — from
A13.)* After B4 (the entry type) and B7 (the `QueryChatHistoryPage` it extends).
Runtime only; the sans-IO `Session` does no file I/O. Grid-agnostic, testable
anywhere. See § Local chat-log files reference (from A13).

- [ ] Add a runtime `chat_log` module: `ChatLogConfig` (default off; per-type
      enable with nearby default off, log dir, filename scheme, timestamp format
      with **seconds default on**, recall window, `conversation.log` on/off +
      retention), `cleanFileName` sanitisation, the four filename schemes
      (optional date suffix, conference MD5 hash), and a writer that appends the
      Firestorm line `[YYYY/MM/DD HH:MM:SS]  Name: message` (local wall-clock,
      `\n␠` continuation, `%3A` colon-escape, `Second Life:` system name).
- [ ] Tap the event stream + own-outbound commands: on `ChatReceived` /
  `InstantMessageReceived` / `GroupSessionMessage` / `ConferenceSessionMessage`
  and our `InstantMessage` / `SendGroupMessage` / `SendConferenceMessage`, write
  the line to the right file when that type's logging is enabled.
- [ ] Extend B7's `QueryChatHistoryPage` runtime handler: when `prev` points
      past the in-memory tail, read the file window (`seek` / `mmap`,
      `LOG_RECALL_SIZE`), parse lines back into `SessionMessage`s (failed parse
      → plain-text fallback; fold space-prefixed continuations), return the page
      + older `prev`.
- [ ] Optional `conversation.log` index: write/update per conversation, purge
  entries older than the retention days on load. Wire `ChatLogConfig` through
  each runtime's constructor at parity; the REPL exposes the toggles.
- [ ] Tests (run anywhere): a logged message writes the exact Firestorm line
      with seconds; each type maps to the right sanitised filename (incl. the
      conference MD5 + legacy-name option); a multi-line message round-trips
      through `\n␠`; read- back parses a stored line into a `SessionMessage` and
      a malformed line into a plain-text fallback; `QueryChatHistoryPage` past
      the tail returns file-backed pages; the `conversation.log` line
      round-trips; default-off writes nothing.

### B10. Persistence/region guard + cross-cutting test suite

*(was old B9 + old B11 — from A9·A11.)* The verification umbrella over the per-
task tests; lands after B1–B9. See § Persistence & region reference (from A9)
and § Test & verification reference (from A11).

- [ ] Add the one-line guard comment at the `begin_handover` `sit` reset
  (`methods.rs:760`) noting chat/presence are grid-level and deliberately not
  reset; confirm `chat_sessions` / `friends` / `online` are seeded empty only in
  `Session::new` and nowhere else (the `const fn` preserved).
- [ ] Persistence tests — the inverse of `teleport_clears_seat`
  (`tests/lifecycle.rs:1716`): seed a session (history + roster + typing) +
  `friends` / `online`, drive `begin_handover` / `promote_child_to_root` /
  `TeleportLocal` / child `DisableSimulator`, assert all unchanged.
- [ ] Logout keep-for-inspection: drive `LogoutReply` (or `close`), assert
  `is_closed()` **and** that `history` / `chat_sessions` / `friends` /
  `is_online` still return the seeded data on the closed session (no read
  accessor gates on `state`); a fresh `Session::new` is empty. No in-place
  clearing on close.
- [ ] Integration suite in `lifecycle.rs`: the per-task state assertions grouped
      by concern (each extending its named existing event test); the
      IM-after-offline invariant. Bidirectional round-trips in `sim_session.rs`
      (sim sends IM / notification / `ChatterBoxInvitation` via `deliver_caps`;
      client store reflects), modelled on
      `friendship_and_calling_cards_reach_client`.
- [ ] Runtime parity checks: the `Arc`-share / no-deep-copy assertion and the
  bevy-direct vs tokio-query data-parity check, plus command-dispatch coverage
  for `AcceptChatInvite` / `DeclineChatInvite` / `MarkSessionRead` / the changed
  `AcceptFriendship` / `JoinSessionVoice` / `LeaveSessionVoice` and the three
  query commands.
