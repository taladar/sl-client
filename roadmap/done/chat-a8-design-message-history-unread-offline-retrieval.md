---
id: chat-a8
title: Design message history, unread & offline retrieval
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A8. Design message history, unread & offline retrieval.** Plan a bounded
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

## History & unread reference (from A8)

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
