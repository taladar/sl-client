---
id: idiomatic-audit-session-facade
title: Every embedder hand-writes the MFA login loop and the three session pumps
topic: idiomatic
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-tokio-caps-refetch]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

The login-with-MFA challenge-retry loop is hand-written **four times**:
`sl-conformance/src/context.rs:510-551`,
`sl-repl-tokio/src/bin/sl-repl-tokio.rs:489`, `sl-client-bevy/src/lib.rs:1015`,
`sl-client-bevy-viewer/src/lib.rs:2447`.

So are the three per-session pump tasks every embedder needs: the
bounded-to-unbounded event forwarder (`context.rs:615-622`), the diagnostics
drain (`:599-607`) and the caps drain (`:579-586`).

Scope: a `sl-client-tokio::Session` facade owning login-with-MFA plus the three
drains. That deletes ~150 lines in `sl-conformance` alone and more elsewhere,
and it is the natural home for the timeout that
[[protocol-audit-tokio-caps-refetch]] adds.

For the record, `sl-conformance`'s `context.rs` does **not** reimplement login —
it correctly uses `Client::connect` plus `client.run(...)`. This is about the
scaffolding around it.
