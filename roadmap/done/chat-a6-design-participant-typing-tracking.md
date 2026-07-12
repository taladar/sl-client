---
id: chat-a6
title: Design participant & typing tracking
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A6. Design participant & typing tracking.** From
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

## Participant & typing reference (from A6)

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
