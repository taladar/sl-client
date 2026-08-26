---
id: protocol-audit-reliable-exhaustion-kills-session
title: One exhausted reliable packet of any kind closes the session as HandshakeFailed
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:5030` calls
`self.close(DisconnectReason::HandshakeFailed)` for **any** reliable packet past
`MAX_RESEND_ATTEMPTS` (6 attempts at a flat 1500 ms, `session.rs:41-43`). A lost
`ObjectSelect` or a chat message therefore tears down the whole session and
reports it as a handshake failure — and `types/session.rs` documents the reason
as "a reliable **handshake** packet".

Two things to separate here: per-message severity (only the handshake-critical
packets should be fatal), and the resend policy itself. The timer never backs
off and never adapts, even though `record_ping_reply` (`circuit.rs:523`)
measures RTT and throws it away. `circuit.rs:398` also starts the retransmit
clock at **enqueue** rather than at transmit (`send()` records `sent_at: now`
while the datagram only enters `self.out`), so a backed-up driver makes every
packet look overdue and triggers spurious resends.
