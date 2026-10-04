---
id: repl-unknown-keyword-arguments-ignored
title: sl-repl silently ignores unknown or misspelled key=value arguments
topic: repl
status: done
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/repl.md](../context/repl.md).

## Observation

`sl-repl/src/args.rs` keeps keyword arguments in a `BTreeMap<String, String>`
and `Registry::build` (`sl-repl/src/registry.rs`) never checks that every
supplied key (or extra positional) was consumed.

## Effect

`send_money_transfer <dest> 10 knd=objectpay` sends a **Gift** (the default
for `kind`); `set_object_media <obj> cleared=3` takes the "any other key" branch
and sets a default media entry on face 0 instead of clearing. Scripted
`--script` runs used as wire probes are as exposed as interactive use.

## Fix

Each `CommandSpec` declares its argument names (and keyword-only status — the
`KEYWORD_ONLY + 100·n` index families become a declared flag); `build` rejects
an unknown key or a surplus positional with a `ReplError` naming it. A test
per command family, plus one that every spec's declared names cover what its
build function reads.

## Resolution

Fixed by tracking reads rather than declaring names: `Args` records every
keyword key and positional slot an accessor reads, and `Registry::build`
refuses whatever a successful build left unread with `ReplError::UnreadArgs`
naming it as typed (a variadic list reads every token from its start). One
mechanism covers every command and cannot drift from what the build functions
read, which is what the planned "declared names cover what build reads" test
existed to catch. The `KEYWORD_ONLY + 100·n` index families stay as they are.

Tests: the two cases above (`knd=`, `cleared=3`), a surplus positional and a
variadic list (`sl-repl/src/registry.rs`); `table_parity`'s
build-from-usage-hint test now covers every command, and fixed two bugs in its
line generator that the refusal exposed (`[key=<v>]` taken as required, a
`key=<v>` field written as a positional).
