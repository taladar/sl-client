---
id: chat-a4
title: Design the session lifecycle (open / join / send / leave / close)
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A4. Design the session lifecycle (open / join / send / leave / close).**
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

## Session-lifecycle reference (from A4)

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
