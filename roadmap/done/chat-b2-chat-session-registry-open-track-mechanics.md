---
id: chat-b2
title: Chat-session registry + open/track mechanics
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B2. Chat-session registry + open/track mechanics — DONE 2026-06-27

*(was old B1 + old B2 + the non-lifecycle half of old B4 — from A1·A2·A4.)* The
typed discriminator, the registry storage/keying, and the get-or-create / remove
mechanics that fill it from the message handlers — but **no** lifecycle state
yet (that is B5). See § Inventory & unified-model reference (from A1),
§ State-model & keying reference (from A2), and § Session-lifecycle reference
(from A4).

- [x] Add the `ChatSessionKind` discriminator —
      `Direct { peer: AgentKey }` / `Group { group_id: GroupKey }` /
      `Conference { id: ImSessionId }`
      (derives incl. `Ord` — it is the map key) in a new chat-session module
      (`session/chat_session.rs`), with the canonical-id helper
      (`ChatSessionKind::canonical_session_id`, **`pub`** so the round-trip is
      testable) reusing `compute_im_session_id` (`conversions.rs:808`). Did
      **not** add the reverse-XOR `direct_peer_from_session_id` helper — no
      consumer keys 1:1 off the XOR id (B3 keys typing by `from_agent_id`).
- [x] Add `ChatSession { last_activity: Instant }` (only this field) +
  `ChatSession::new(now)` (no `Default` — `Instant` has none; `const fn`). Added
  the private `chat_sessions: BTreeMap<ChatSessionKind, ChatSession>` to
  `Session`, const-empty in the `const fn` constructor beside `online`.
- [x] Added only the `chat_session_mut(kind, now)` get-or-create helper (the
  read-only `chat_session` / non-creating `chat_session_get_mut` land with their
  first consumer in B3).
- [x] Fold get-or-create into outbound `start_group_session` /
      `start_conference` / `send_group_message` / `send_conference_message` /
      `send_instant_message` (1:1 opens here) and inbound group/conf
      `SessionSend` + the participant arms + the new explicit 1:1 `Message` arm
      (only dialog 0 opens a Direct session — other non-session dialogs do not);
      `leave_group_session` / `leave_conference` **remove** the entry; 1:1 is
      never removed.
- [x] Public `chat_sessions() -> impl Iterator<Item = ChatSessionKind>` lister,
  ordered by `last_activity` **newest-first** (kind breaks ties; so the field is
  read).
- [x] **Persistence guard (from A9):** added **no** `chat_sessions` clear at the
  four region-boundary sites (`begin_handover`, `promote_child_to_root`,
  `TeleportLocal`, child `DisableSimulator`); the store is grid-level. (Tests
  live in B10.)
- [x] Unit tests (11 new, `tests/lifecycle.rs`): open per kind (outbound +
  inbound), a non-session dialog opens nothing, `leave_*` removes, 1:1 persists,
  creates-once + restamps `last_activity` (observed via the newest-first
  ordering), the canonical-id round-trip per kind.
