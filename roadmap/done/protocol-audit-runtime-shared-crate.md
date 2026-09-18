---
id: protocol-audit-runtime-shared-crate
title: 1677 byte-identical lines are duplicated between the two runtime crates
topic: protocol
status: done
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

## Done

All four modules — the three byte-identical ones plus `retry.rs` — now live
once, in the new **`sl-client-common`** crate that both runtimes depend on.
1,782 lines stopped being two copies. The move was mechanical: each moved file
is byte-identical to *both* former copies apart from `pub(crate)` → `pub`, the
ten `#[must_use]` attributes that widening to `pub` requires, and `retry.rs`'s
module doc, which had been the one place the two copies disagreed and is now
written for both runtimes (it decides *whether* and *how long*; awaiting the
`Duration` on a tokio timer or blocking a task-pool thread stays with the
caller).

The 18 tests that used to run twice now run once, in the crate that owns the
code: `sl-client-tokio`'s test count went from 22 to 4, which is what it
genuinely had of its own.

Falling out of the move, four dependencies were left with no user and were
dropped from the runtime manifests: `flate2` and `time` from both, and `sl-wire`
and `fs-err` from `sl-client-bevy` (the latter down to a dev-dependency there,
for the `avatar_replay` example). `sl-wire` is the telling one — the bevy
runtime's only use of the codec crate was a `Llsd` in a copied *test*. What
stayed duplicated is `http_proxy.rs`, which really is runtime-specific.
