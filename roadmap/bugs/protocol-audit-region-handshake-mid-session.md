---
id: protocol-audit-region-handshake-mid-session
title: A root RegionHandshake outside AwaitingHandshake is silently dropped
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:2798` wraps the **entire** `RegionHandshake`
arm — `send_region_handshake_reply`, `note_region_flags` and the
`Event::RegionInfoHandshake` push — inside
`if matches!(self.state, SessionState::AwaitingHandshake)`.

A simulator re-sends `RegionHandshake` while the session is `Active` on a region
restart, an estate change or a terrain-texture change. Nothing replies, so the
sim keeps retrying, and the region flags never refresh.

The **child** arm at `:1957` replies and updates flags unconditionally. The two
are hand-copied dispatchers that have drifted — see
[[protocol-audit-dispatch-child-drift]] for the structural fix that stops the
next one.
