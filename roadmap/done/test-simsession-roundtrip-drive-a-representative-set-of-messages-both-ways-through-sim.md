---
id: test-simsession-roundtrip
title: drive a representative set of messages both ways through SimSession an
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 20 — Server side (SimSession) — stretch, no grid
---

Context: [context/test.md](../context/test.md).

Optional final tier: in-process client ↔ `SimSession` round-trips for messages
that are hard to provoke against a live grid. Complements
`sl-proto/tests/sim_session.rs`. These are not grid-gated.

`simsession-roundtrip` — drive a representative set of messages both ways
through `SimSession` and assert symmetric decode/encode.

---

**Done (2026-08-21).** `sl-proto/tests/sim_session_symmetry.rs`, built on
the same in-memory `Session` ↔ `SimSession` pump as
`sl-proto/tests/sim_session.rs`:

- **Client → simulator, raw-forwarded:** seven family tests (inventory,
  groups, object edits, parcels/region, profile/picks/classifieds,
  money/mutes, appearance/misc) send every client message that
  `SimSession` surfaces as `ServerEvent::ClientMessage`, assert the
  simulator's decode equals an independent decode of the same datagram,
  and that re-encoding the surfaced message reproduces the (zero-code
  expanded) wire body byte-for-byte. The `RAW_FORWARDED` ledger (98
  messages) pins the set, as the message-level sibling of
  `SESSION_FLOW_COVERAGE`; a future typed arm for one of them fails its
  family test so the ledger edit is deliberate.
- **Client → simulator, typed:** the previously unasserted UDP server
  events — `RegionHandshakeReplied`, `AgentUpdate`, `DropAttachments`,
  single `RezAttachment`, `SpinObjectStart/Update/Stop`,
  `DuplicateObjectsOnRay`, `TeleportViaLure`.
- **Simulator → client:** the three senders no loopback test exercised —
  `send_offline_notification`, `send_parcel_overlay_chunk`,
  `enqueue_chatterbox_agent_list_updates` (over the CAPS event queue).

With these, every `SimSession::send_*`/`enqueue_*` is exercised by one of
the two loopback files (the audit grep is in the file header).
