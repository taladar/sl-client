---
id: viewer-audit-rlv-behaviour-table-test
title: Pin the whole RLV behaviour table with one table-driven test
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-rlv-behaviour-lookup]
---

Context: [context/viewer.md](../context/viewer.md).

`rlv_behaviours!` (`sl-rlv/src/behaviour.rs:17`) declares roughly 175 keywords;
the tests assert about 20 specific ones.

Add a test that iterates **every declared row** and asserts:

- `from_keyword(kw) == Some(variant)` and `variant.keyword() == Some(kw)`;
- no two rows share a keyword;
- `has_strict()` matches the table's third column;
- `parse_field(&format!("{kw}_sec=n"))` yields `strict == has_strict()`, and
  `Unknown` otherwise.

Also missing, and this crate needs it more than most because its input is
untrusted in-world chat: a robustness test over hostile owner-say lines
(unbalanced separators, very long option strings, non-UTF-8-adjacent content,
deeply repeated commas).

Once [[viewer-audit-rlv-behaviour-lookup]] adds a param-type column, extend the
table test to assert a force-only keyword used as a restriction resolves to
`Unknown`.
