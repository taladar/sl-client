---
id: protocol-audit-llsd-recursion-depth-cap
title: LLSD parsers recurse with no depth limit
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/protocol.md](../context/protocol.md).

The three LLSD parsers recurse through `parse_value` -> `parse_array` /
`parse_map` -> `parse_value` with **no depth counter anywhere in the crate**
(`sl-llsd/src/notation.rs:293`, `sl-llsd/src/binary.rs:358`,
`sl-llsd/src/value.rs:539`; a search for `depth` / `MAX_NEST` over `sl-llsd/src`
and `sl-wire/src/llsd.rs` finds only a doc comment).

Notation costs **one byte of input per nesting level** (`[[[[[...`), binary
five, XML about seven. A ~100 KB CAPS response or mesh header therefore
overflows the stack. This is unauthenticated, remote, and **not** a catchable
panic — the process dies.

Scope: thread a depth parameter through the three `parse_value` entry points and
fail with a new `LlsdError` variant past a cap. Cover all three encodings; the
notation walker `Scan::skip_value` needs the same guard. Pair with
[[protocol-audit-decoder-fuzz-harness]], which is what pins it afterwards.
