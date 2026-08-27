---
id: protocol-audit-llsd-recursion-depth-cap
title: LLSD parsers recurse with no depth limit
topic: protocol
status: done
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

## Fixed (2026-08-27)

`MAX_NESTING_DEPTH` (128) in `sl-llsd`, with a new
`LlsdError::NestingTooDeep`. Enforced on the way *in*, before recursing,
because a stack overflow aborts the process rather than raising an error the
type could carry. All four recursion sites are covered: the binary parser
(depth on `Cursor`), the notation parser (depth on `NotationParser`), the
public `Scan::skip_value`, which recurses into itself for `[` and `{` and
reports the refusal through its own `None` channel, and the XML walk.

The limit is far above anything the protocol produces — a mesh header nests
twice, the deepest CAPS bodies single digits — and far below what overflows a
thread stack. The reference threads an equivalent `max_depth` through
`doParse` / `parseMap` / `parseArray` and fails at zero, but defaults it to
`-1` (unlimited), so the concrete ceiling is ours.

**The XML path needed more than a guard on our own walk.** roxmltree's element
parsing recurses and overflows the stack between roughly 1000 and 2000 levels,
so the crash happened inside the dependency before we were handed a tree —
measured, not assumed. Its own `depth` field bounds *entity references* (the
billion-laughs case) and `nodes_limit` bounds node **count**, which a
deep-but-narrow document never approaches, so neither covers this.
`parse_llsd_xml` now runs a byte-level `nesting_within` pre-scan before calling
roxmltree, skipping comments, CDATA, processing instructions and the doctype,
and treating `<x/>` as opening nothing. The guard in `node_to_llsd` stays as
the backstop for that being a scan rather than a parser.

Eight tests. Three of them — 100_000 nested arrays in binary, 100_000 `[` in
notation, 4_000 nested elements in XML — abort the process without the fix
rather than failing. They are cheap despite the input size, because the parse
bails at depth 128 without reading the rest.

**Follow-up raised:** `sl-wire` parses XML-RPC login responses and CAPS bodies
through roxmltree directly, not through `sl-llsd`, so those paths still have
the upstream exposure — see [[protocol-audit-roxmltree-nesting-in-sl-wire]].
