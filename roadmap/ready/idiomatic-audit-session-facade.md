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

The login-with-MFA challenge-retry loop is hand-written in every embedder
(locations as of 2026-10-15): `sl-conformance/src/context.rs` (two arms on
`Error::MfaChallenge`, around lines 662 and 818),
`sl-repl-tokio/src/bin/sl-repl-tokio.rs:482`, `sl-client-bevy/src/lib.rs`
(`SlMfaChallenge`, around 1081 and 1296) and the viewer's
`capture_login_outcome` restart loop (`sl-client-bevy-viewer/src/assembly.rs`).

So are the per-session pump tasks every embedder needs, all in
`context.rs`'s connect path (around lines 955-1010): the bounded-to-unbounded
event forwarder, the neighbour drain, the caps drain and the diagnostics
drain.

Scope: a `sl-client-tokio::Session` facade owning login-with-MFA plus the
drains. That deletes ~150 lines in `sl-conformance` alone and more elsewhere,
and it is the natural home for the timeout that
[[protocol-audit-tokio-caps-refetch]] adds.

For the record, `sl-conformance`'s `context.rs` does **not** reimplement login —
it correctly uses `Client::connect` plus `client.run(...)`. This is about the
scaffolding around it.
