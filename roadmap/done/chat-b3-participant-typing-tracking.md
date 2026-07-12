---
id: chat-b3
title: Participant & typing tracking
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B3. Participant & typing tracking — DONE 2026-06-27

*(was old B6 — from A6.)* Adds the roster + typing collections, filled at B2's
handler arms and read through accessors. See § Participant & typing reference
(from A6).

- [x] Added `participants: BTreeSet<AgentKey>` + `typing: BTreeMap<AgentKey,
  Instant>` to `ChatSession` (both `const`-empty in `ChatSession::new`, so the
  constructor stays `const fn`); added the `TYPING_TIMEOUT: Duration = 9s`
  `pub(crate)` constant in `chat_session.rs`; added the non-creating
  `chat_session_get_mut(kind)` and read-only `chat_session(kind)` helpers beside
  B2's `chat_session_mut`.
- [x] Roster fold: in the group / conference `SessionAdd` / `SessionLeave`
  participant arms, insert/remove `agent_id` by `joined` on the session returned
  by `chat_session_mut` (same get-or-create B2 uses — participant traffic also
  opens the session, the A4 rule).
- [x] Typing fold: in the `TypingStart` / `TypingStop` arm, resolve the session
  (a tracked `Group` / `Conference` keyed by `block.id`, else `Direct { peer:
  from_agent_id }`) via `chat_session_get_mut` (no create); `from_agent_id →
  now` on start, remove on stop. Keying 1:1 by `from_agent_id` (the peer), not
  the wire XOR id.
- [x] Expiry: in `run_timeout(now)` (the session's timed loop), prune typing
  entries whose last-seen is `>= TYPING_TIMEOUT` old via `BTreeMap::retain` +
  `saturating_duration_since`, keeping the read accessor `now`-free.
- [x] Accessors `participants(session)` / `typing(session)` (Direct participants
  synthesised as `{ peer }` straight from the key; group / conference read the
  stored roster). `send_im_typing` unchanged (no self tracking).
- [x] Tests (6 new, `tests/lifecycle.rs`): `SessionAdd` / `SessionLeave`
  insert/remove + open the session; `participants(Direct)` = `{peer}`;
  `ImTyping` start/stop on a 1:1 keyed by `from_agent_id`; typing does **not**
  open a session; entry survives a pre-timeout tick then expires at
  `TYPING_TIMEOUT`; `TypingStop` clears immediately; group typing keys by
  `block.id`.
