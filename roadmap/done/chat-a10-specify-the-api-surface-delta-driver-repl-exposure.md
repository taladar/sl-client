---
id: chat-a10
title: Specify the API-surface delta & driver/REPL exposure
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A10. Specify the API-surface delta & driver/REPL exposure.** Enumerate
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

## API-surface & exposure reference (from A10)

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
