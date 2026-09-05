---
id: viewer-audit-rlv-behaviour-table-test
title: Pin the whole RLV behaviour table with one table-driven test
topic: viewer
status: done
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

## Done

Three table-driven sweeps in `sl-rlv/src/lib.rs`, all walking
`RlvBehaviour::ALL` (a new associated const the `rlv_behaviours!` macro emits,
which is also the enumeration `@getcommand` will want):

- `every_table_row_roundtrips` — `from_keyword(kw) == Some(variant)` and
  `variant.keyword() == Some(kw)` for every row, no keyword declared twice, and
  every row declares at least one param kind and never the same one twice. A row
  with an empty kind set could never resolve, so that is a real trap to close.
- `every_table_row_answers_only_its_own_param_kinds` — for each of the four
  kinds, decode `<kw>=n` / `<kw>=force` / `<kw>=1234` / the bare keyword and
  assert the behaviour is the row exactly when the row declares that kind and
  `Unknown` otherwise, that the keyword survives, that the param classified as
  the kind asked for, and that nothing came back as a modifier command. Also
  that a bare keyword is `MissingParam` for everything but `@clear`. Two probes
  are skipped as unreachable rather than asserted wrongly: a param-less field is
  a syntax error for anything but `@clear`, and `@clear=force` / `@clear=1234`
  are *filtered clears*, not a force or a reply.
- `every_table_row_answers_the_strict_suffix_it_declares` — `<kw>_sec=n` yields
  `strict` exactly when the row has it, `Unknown` otherwise, and keeps the
  suffix on `keyword`.

Plus `local_modifier_table_roundtrips` over `RlvLocalModifier::ALL`, which also
asserts every modifier hangs off a restriction — the only rows the reference
registers modifiers on.

The hostile-input test the task asked for is
`hostile_owner_say_lines_are_decoded_without_panicking`: 23 lines covering a
bare `@`, 4096 repeated commas with and without a prefix, `:::=:::`, `====`,
a lone `_sec`, a trailing `_`, an 8192-character option, a 2048-fold `a_`
keyword, i32 overflow in a reply channel both ways, a leading `+`, a leading
space, non-ASCII in both keyword and option (so a multi-byte character is never
split by the ASCII lower-casing), and embedded NULs. Every line must decode to
`Ok` or `Err` — never a panic — and whatever survives has to be internally
consistent: an `Unknown` behaviour is never reported as strict or as a modifier,
and an option is never `Some("")`.

Not a unit test, but worth recording: the table's *content* was cross-checked
mechanically against `RlvBehaviourDictionary`'s constructor rather than by eye —
see the Verified section of [[viewer-audit-rlv-behaviour-lookup]]. All 176
shared keywords, all 19 strict flags and both local-modifier lists agree.
