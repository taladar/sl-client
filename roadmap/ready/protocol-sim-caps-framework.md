---
id: protocol-sim-caps-framework
title: Server-side CAPS core — seed, EventQueueGet, SimCaps dispatch
topic: protocol
status: ready
origin: user request (2026-07) — complete simulator protocol surface
points: 8
refs: [viewer-fake-grid]
---

Context: [context/protocol.md](../context/protocol.md).

The client requests ~58 capabilities (`REQUESTED_CAPABILITIES`,
`sl-proto/src/session.rs`), and sl-wire holds client-direction
`build_*_request`/`parse_*_response` pairs for them spread across its
modules — but server-direction CAPS support today is only
`build_event_queue_response` + `SimSession::enqueue_caps_event`. Build the
sans-I/O server-side CAPS core in sl-wire/sl-proto:

- parse a seed-capability request (the client POSTs the list of wanted cap
  names — `build_seed_request`'s inverse) and build the granting URL-map
  response;
- parse `EventQueueGet` requests (id/ack/done semantics, timeout re-poll)
  pairing with the existing `build_event_queue_response`;
- a `SimCaps` dispatch registry (cap name → handler) integrated with
  `SimSession::enqueue_caps_event`;
- establish the inverse-pairing convention: every client-direction
  `build_*_request`/`parse_*_response` pair gains its
  `parse_*_request`/`build_*_response` inverse;
- commit a coverage table over all `REQUESTED_CAPABILITIES` entries
  (pinned-table convention) that the cluster tasks tick off.

Verified in-memory against the client's own
`build_seed_request`/`parse_seed_response` round-trip. This is the
foundation both for [[viewer-fake-grid]] and for a complete simulator
protocol surface; the world-authority grid itself stays out of scope.
