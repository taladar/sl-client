---
id: repl-unknown-keyword-arguments-ignored
title: sl-repl silently ignores unknown or misspelled key=value arguments
topic: repl
status: bugs
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
