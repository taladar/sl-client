---
id: chat-b6
title: Presence-driven auto-reset
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B6. Presence-driven auto-reset — DONE 2026-06-27

*(was old B7 — from A7.)* The sole coupling between presence (B1) and the
registry (B2/B3). See § Presence-driven reset reference (from A7).

- [x] In the `OfflineNotification` handler (`methods.rs`), after B1's
  `online.remove(friend)`, for each offlined agent iterate `chat_sessions` and
  `typing.remove(agent)` + `participants.remove(agent)` (convert `FriendKey` →
  `AgentKey` via the shared `Key`: `AgentKey(id.0)`). No session is removed and
  no per-session "offline" marker is stored (a 1:1 persists to logout; its
  peer-offline state is `!is_online(peer)`).
- [x] `FriendsOnline` (`OnlineNotification`) gains **no** chat code. No session
  removal, no lifecycle change, no stored offline marker, no new `Event`.
- [x] Tests (3 new, `tests/lifecycle.rs`): seed a conference roster + a 1:1
      typing entry for a friend, deliver `OfflineNotification`, assert they are
      gone from roster + `typing` **but** the sessions still exist and
      `is_online` is false; a non-friend participant is untouched (only
      `SessionLeave` removes them); `OnlineNotification` re-adds, changes no
      session.

  **Done — see § Presence-driven reset reference (from A7). The new sans-IO
  tests live in `lifecycle.rs`:
  `offline_notification_clears_typing_and_roster_keeping_sessions`,
  `offline_notification_leaves_other_participants_untouched`,
  `online_notification_changes_no_session`. Pure sans-IO fold (no `Command` /
  `Event` / accessor delta), so no runtime-parity work — B7 adds the read API.**
