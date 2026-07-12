---
id: chat-b7
title: Chat read-model exposure + the query/page API
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B7. Chat read-model exposure + the query/page API — DONE 2026-06-27

*(was old B10 — from A10.)* Consolidates the aggregate view + the divergent
runtime exposure (bevy direct borrow; tokio/REPL pull). See § API-surface &
exposure reference (from A10).

- [x] Added the view types `ChatSessionInfo` / `ChatLifecycleView` /
      `FriendPresence` / opaque `MessageCursor` (all `Clone + Debug`;
      `FriendPresence` / `MessageCursor` `Copy`) in `chat_session.rs`
      (re-exported via `session.rs` + crate root), plus the snapshot-builders
      `chat_sessions_info()` / `friends_presence()` / `history_page(session,
      before, limit)` on `Session`, composing the B1–B5 primitives. The list
      builder orders newest-first by `last_activity` (reusing `chat_sessions()`)
      and omits history; `ChatLifecycleView` flattens `Invited` inline.
- [x] Added query `Command`s `QueryChatSessions` / `QueryChatHistoryPage {
  session, before, limit }` / `QueryFriends` and the reply `Event`s
  `ChatSessions(Arc<[ChatSessionInfo]>)` / `ChatHistoryPage { session, messages:
  Arc<[SessionMessage]>, prev }` / `FriendsSnapshot(Arc<[FriendPresence]>)` —
  `Arc<[…]>` payloads, built once and `Arc`-cloned across the channel, never
  deep `Vec` copies.
- [x] tokio: handles the three queries by calling the builder and `events.send`
  (mirroring the `QueryScriptPermissions` arm); no wire send.
- [x] bevy: accepts the same query commands for parity, surfacing the reply via
  `events.write` — a bevy *system* may instead borrow `&Session` and call the
  builders directly (zero round-trip), which the builders are designed for.
- [x] REPL: registered `query_chat_sessions` / `query_chat_history_page`
  (`[limit]`, default 50; the opaque cursor means it always fetches the newest
  page) / `query_friends` in `registry.rs`; added the three reply `event_name`
  arms + the three `command_name` arms in `format.rs`.
- [x] Tests (5 new, `tests/lifecycle.rs`): `chat_sessions_info` is
      newest-first, `Invited` flattened, no history; `history_page` is bounded
      newest-first + `prev`, paging older windows in bounded steps to `None` at
      the oldest; `history_page` on an unopened session is empty;
      `friends_presence` online flag per friend; the `ChatSessions` payload is
      an `Arc<[…]>` that `Arc`-clones (pointer-equal) and equals the direct
      builder read (data parity, for the bevy-direct vs tokio-reply equality).

  **Refinement (not a behavioural deviation).** `history_page` returns
  `(impl Iterator<Item = &SessionMessage>, Option<MessageCursor>)` rather than
  the reference's literal `(&[SessionMessage], …)`: the B4 history store is a
  `VecDeque`, so a `&self` borrow cannot hand back a single contiguous slice
  *reordered* newest-first — an iterator of borrows is the faithful zero-copy
  realisation (the sibling builders `chat_sessions_info` / `friends_presence`
  already return `impl Iterator`). All observable behaviour — newest-first
  bounded pages, the opaque `prev` cursor, zero-copy borrow on bevy, one bounded
  `Arc` window on the channel runtimes — is as specified. `MessageCursor` is
  opaque (a private "newest consumed" count); A13 reworks its innards for the
  on-disk archive boundary.
