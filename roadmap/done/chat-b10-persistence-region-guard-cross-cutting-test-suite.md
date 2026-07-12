---
id: chat-b10
title: Persistence/region guard + cross-cutting test suite
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B10. Persistence/region guard + cross-cutting test suite — DONE 2026-06-27

*(was old B9 + old B11 — from A9·A11.)* The verification umbrella over the per-
task tests; lands after B1–B9. See § Persistence & region reference (from A9)
and § Test & verification reference (from A11).

- [x] Add the one-line guard comment at the `begin_handover` `sit` reset
  noting chat/presence are grid-level and deliberately not reset; confirmed
  `chat_sessions` / `friends` / `online` are seeded empty only in `Session::new`
  and nowhere else (the `const fn` preserved). *(The comment landed beside the
  `drop_inworld_grants()` call in `begin_handover`; the `Session::new` seeding
  is unchanged at the three `BTreeMap::new()` / `BTreeSet::new()` slots.)*
- [x] Persistence tests — the inverse of `teleport_clears_seat`: a shared
  `seed_chat_and_presence` fixture seeds a 1:1 (history + unread) + a conference
  roster + a 1:1 typer + a buddy online, and four tests
  (`teleport_preserves_chat_and_presence` via `TeleportFinish`/`begin_handover`,
  `neighbour_crossing_…` via `CrossedRegion`/`promote_child_to_root`,
  `local_teleport_…` via `TeleportLocal`, `disable_simulator_…` via a child
  `DisableSimulator`) drive each reset site and assert every store unchanged.
- [x] Logout keep-for-inspection: `logout_keeps_chat_and_presence_readable`
  drives `LogoutReply`, asserts `is_closed()` **and** that `history` /
  `chat_sessions` / `friends` / `is_online` still return the seeded data on the
  closed session (no read accessor gates on `state`); a fresh `Session::new` is
  empty. No in-place clearing on close.
- [x] Integration suite: the per-task state assertions grouped by concern were
  shipped by B1–B9 (each extending its named existing event test), and the
  IM-after-offline invariant is `im_after_offline_does_not_resurrect_presence`
  (already present). New bidirectional round-trips in `sim_session.rs`
  (`inbound_instant_message_…`, `inbound_presence_notifications_…`,
  `inbound_chatterbox_invitation_reaches_client_store`) have a real
  `SimSession` send an IM / presence notification / `ChatterBoxInvitation` (via
  `enqueue_caps_event` + `deliver_caps`) and assert the client store reflects
  it, modelled on `friendship_and_calling_cards_reach_client`.
- [x] Runtime parity checks: the `Arc`-share / no-deep-copy plus the
  bevy-direct vs tokio-query data-parity assertion exists for `ChatSessions`
  (B7's `chat_sessions_reply_shares_an_arc_without_deep_copy`); B10 adds the
  `Friends` and `ChatHistoryPage` counterparts
  (`friends_and_history_replies_share_an_arc_without_deep_copy`), covering all
  three query replies. **Refinement (documented deviation):** the per-command
  *dispatch* coverage for `AcceptChatInvite` / `DeclineChatInvite` /
  `MarkSessionRead` / `AcceptFriendship` / `JoinSessionVoice` /
  `LeaveSessionVoice` is exercised at the sans-IO `Session` accessor/mutator
  level in `lifecycle.rs` (where the pure-state command halves live and are
  tested), **not** as new tests inside the `sl-client-tokio` / `sl-client-bevy`
  crates. Those crates carry no test harness (a deliberate, pre-existing state
  — B7, which the A11 case map assigned "crate command-dispatch tests", was
  completed the same way): the dispatch arms run inside `Client::run` (which
  consumes `self` into a spawned task) and are thin wrappers that call the
  tested `Session` methods plus fire-and-forget CAPS HTTP side-effects, so
  isolating one command would need a mock UDP+HTTP harness disproportionate to
  the thin glue it would cover.
