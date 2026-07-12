---
id: chat-b4
title: Per-session history & unread
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B4. Per-session history & unread — DONE 2026-06-27

*(was old B8, with old-B13's `SessionMessage` rename applied up front — from
A8·A13.)* Uses the non-colliding name `SessionMessage` from the start (the
existing nearby-chat `ChatMessage` lives at `types/chat.rs:254`). See § History
& unread reference (from A8) and § Local chat-log files reference (from A13).

- [x] Added the public `SessionMessage { sender: AgentKey, dialog: ImDialog,
  text: String, timestamp: Option<u32> }` in `chat_session.rs` (re-exported via
  `session.rs` + crate root); added `history: VecDeque<SessionMessage>` +
  `unread: u32` to `ChatSession` (both `const`-empty, constructor stays
  `const fn`); added `HISTORY_CAP = 256` `pub(crate)` constant plus the
  `ChatSession::log_inbound` / `log_outbound` / `push_history` helpers (the
  cap-prune lives in `push_history`).
- [x] Inbound log: in the group / conference `SessionSend` arms and the 1:1
  `Message` arm, build a `SessionMessage` (wire `timestamp` via
  `optional_u32_from_wire`) and route through the new
  `Session::log_inbound_message` (get-or-create + `log_inbound`, which bumps
  `unread` unless `sender == self`, then prunes). Offline replays drain through
  the CAPS `ReadOfflineMsgs` arm into
  the `Direct { from_agent_id }` session with their original wire `timestamp`
  (only `Message` dialogs are logged; the legacy UDP offline path rides the same
  1:1 `Message` arm).
- [x] Outbound log: `send_instant_message` / `send_group_message` /
  `send_conference_message` capture the own agent id and call
  `Session::log_outbound_message` (`{ sender: self, timestamp: None, .. }`,
  `unread = 0`).
- [x] Added `Session::mark_session_read(session)` + `Command::MarkSessionRead {
  session: ChatSessionKind }` wired at parity (tokio / bevy dispatch + REPL
  `mark_session_read` spec with a `build_chat_session_kind` helper + `format.rs`
  command name), resetting `unread`.
- [x] Accessors `history(session)` (oldest-first refs) / `unread(session)` /
  `total_unread()` (saturating sum).
- [x] Tests (7 new, `tests/lifecycle.rs`): inbound 1:1 logs `unread == 1`; group
  `SessionSend` logs to the group session; own outbound logs (`sender = self`,
  `timestamp = None`) + resets unread; `mark_session_read` resets while keeping
  history; `HISTORY_CAP + 1` drops the oldest; an `offline == true` IM logs its
  wire `timestamp` + bumps `unread`; `total_unread` sums + drops on read.
