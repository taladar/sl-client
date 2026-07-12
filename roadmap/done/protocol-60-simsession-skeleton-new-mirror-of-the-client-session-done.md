---
id: protocol-60
title: SimSession skeleton (new; mirror of the client Session). DONE
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**60. `SimSession` skeleton (new; mirror of the client `Session`). ✅ DONE.** A
sans-I/O `SimSession` in `sl-proto/src/sim_session.rs` (exported with
`ServerEvent` and `AgentUpdateInfo`) that accepts a circuit (`UseCircuitCode` +
`CompleteAgentMovement`, replying `AgentMovementComplete`), tracks sequence /
pending acks / a seen-window / reliable retransmission / inactivity & ack-flush
timers (reusing the symmetric `sl-wire` `encode_datagram`/`parse_datagram`/
`PacketFlags`/`PacketAck` machinery — the bookkeeping mirrors the client
`Circuit`), answers `StartPingCheck` with `CompletePingCheck` and pings the
client itself on a 5 s cadence, handles `LogoutRequest` (→ `LogoutReply`,
close), exposes a typed push API (`push(&AnyMessage, Reliability)` for the
general server→client messages — `RegionHandshake`/`ObjectUpdate`/`LayerData`/…
— plus
`send_chat_from_simulator` and `start_ping_check`), enqueues CAPS events
(`enqueue_caps_event` + `take_event_queue_response` building #59's
`build_event_queue_response`), and decodes the client-only messages into a
`ServerEvent` enum (the inverse of the client `Command`/`Event` split): typed
`CircuitOpened`/`AgentArrived`/`RegionHandshakeReplied`/`PingRequested`/
`Throttle`/`AgentUpdate`/`Chat`/`InstantMessage`/`LoggedOut`/`Disconnected`
variants for the lifecycle and high-value payloads (reusing the client's
`instant_message` decoder, now `pub(crate)`; a new
`Throttle::from_bits_per_second` inverse of `bits_per_second` decodes
`AgentThrottle`), with every other decoded client message surfaced verbatim as
`ServerEvent::ClientMessage(Box<AnyMessage>)`.
12 in-memory loopback tests in `sl-proto/tests/sim_session.rs` drive a
`SimSession` and a client `Session` against each other through the real
framing/ack/zerocode path (circuit setup→arrival, chat both directions, IM,
throttle, ping both directions, clean logout, reliable-ack flush, inactivity
timeout, CAPS EventQueue round-trip, and `ClientMessage` fall-through). Same
doc-link gotcha as #54–#59 (the public `SimSession` docs avoid intra-doc links
to the private `SimState`/the module `self`, else `cargo doc
-D private_intra_doc_links` fails). **Next = #61/F10 (done — see #61).**
