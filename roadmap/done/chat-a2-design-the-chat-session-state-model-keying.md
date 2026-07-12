---
id: chat-a2
title: Design the chat-session state model & keying
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A2. Design the chat-session state model & keying.** Specify what
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

## State-model & keying reference (from A2)

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
