---
id: test-reliable-retransmit
title: exercise reliable resend under loss
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 19 — Error handling & recovery `[both]`
refs: [test-handover-mock-grid-harness]
---

Context: [context/test.md](../context/test.md).

`reliable-retransmit` — exercise reliable resend under loss. `1av`.
**Resolved in-process (no live conformance case).** Investigated 2026-08-12:

Loss cannot be induced on a live circuit. The production runtime has no
fault-injection seam (a bare `tokio::net::UdpSocket`, deliberately no
drop/delay knobs), the local grid runs over loopback where organic loss is
effectively nil, and interposing a lossy proxy would require rewriting the
sim address inside the login response. This is the same conclusion the
[[test-handover-mock-grid-harness]] item drew for timeouts and
lost-message races: behaviour that needs deterministic loss is exercised
against the sans-IO `Session` in-process, not against a live grid.

What the item asked for is now deterministically covered in-process,
driving the **real** framing/ack/resend code on both peers:

- `sl-proto/tests/lifecycle.rs`
  `retransmits_unacknowledged_reliable_packets` (upgraded): the resend
  timer re-emits an unacked reliable packet and the retransmission now
  **asserts the `RESENT` wire flag** (and `RELIABLE`), which no test
  checked before.
- `sl-proto/tests/lifecycle.rs`
  `exhausted_resend_reports_expected_reply_missing` /
  `exhausted_resend_is_silent_without_diagnostics` (pre-existing): budget
  exhaustion (6 × 1.5 s ≈ 9 s) emits
  `Diagnostic::ExpectedReplyMissing { sequence: Some(_) }` and closes the
  root circuit.
- `sl-proto/tests/sim_session.rs`
  `client_reliable_resend_survives_loss_and_sim_deduplicates` (new): a
  client chat datagram's first transmission is dropped; the retransmit
  reuses the sequence, carries `RESENT`, dispatches exactly once on the
  simulator, and a duplicate delivery of the same datagram is acked but
  **not** re-dispatched (the first-ever exercise of the inbound
  seen-window dedup).
- `sl-proto/tests/sim_session.rs`
  `sim_reliable_resend_survives_loss_and_client_deduplicates` (new): the
  mirror direction — a lost simulator `AlertMessage` is retransmitted
  with `RESENT`, surfaces exactly one client event, and a duplicate
  delivery is not surfaced twice.

The **live** half of the reliable path is already covered by existing
green cases: `throttle-set` proves a reply-less reliable send was acked by
watching root keep-alive pings past the ~9 s retransmit budget and
asserting no `ExpectedReplyMissing` diagnostic (acceptance-by-absence),
and `keepalive-ping` covers the underlying circuit-health signal. A
dedicated `reliable-retransmit` grid case would either duplicate
`throttle-set` (healthy path) or assert nothing (loss never occurs), so
none is added — the first Phase 19 item resolved without a records entry.
