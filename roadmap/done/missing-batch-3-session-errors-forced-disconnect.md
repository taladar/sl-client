---
id: missing-batch-3
title: session errors & forced disconnect
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

## Batch 3 — session errors & forced disconnect

`Error` (Low 423), `FeatureDisabled` (Low 19): surface as typed error events.
`KickUser` (Low 163): surface as a kick event and drive the session toward
`Event::Disconnected`/`LoggedOut`.

Implemented in `types/server_error.rs` as `Event::ServerError(Box<ServerError>)`
(HTTP-like `code`, originating `system` path, human-readable `message`, plus the
deliberately-polymorphic `id` correlation field kept as a raw `Uuid` and the
binary LLSD `data` blob kept verbatim), `Event::FeatureDisabled(FeatureDisabled
{ message, agent: AgentKey, transaction: TransactionId })`, and
`Event::Kicked(Kick { agent: AgentKey, reason: String })`. The `KickUser` arm
also calls `self.close(DisconnectReason::Kicked { message })` — a new
`DisconnectReason` variant — so the session reaches its terminal
`Event::Disconnected` state; the `KickUser` routing fields (target sim address,
echoed session id) carry nothing the client needs and are dropped.
