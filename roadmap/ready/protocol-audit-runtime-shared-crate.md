---
id: protocol-audit-runtime-shared-crate
title: 1677 byte-identical lines are duplicated between the two runtime crates
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-runtime-parity-gaps]
---

Context: [context/protocol.md](../context/protocol.md).

`diff sl-client-tokio/src/X sl-client-bevy/src/X` is **empty** for three files:
`chat_log.rs` (1051 lines, 12 tests), `inventory_cache.rs` (442, 3 tests) and
`lsl_syntax_cache.rs` (184, 1 test) — 1677 lines total. `retry.rs` differs by 8
lines of doc comment. (`http_proxy.rs` differs by 61 and is genuinely
runtime-specific.)

None of the duplicated code is runtime-specific: it is synchronous `fs` plus
pure logic. Today every fix and every test has to be applied twice, and 19 of
`sl-client-tokio`'s 22 unit tests are byte-identical copies of the bevy ones —
so the tokio runtime has effectively three tests of its own.

This is the largest violation of the parity rule's *spirit*: parity is currently
maintained by **copying**, which is one missed paste from silently diverging.
Extract the three modules into a shared crate that both runtimes depend on.
