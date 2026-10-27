---
id: server-lsl-lib-strings-lists
title: Library tranche — strings, lists, encoding and hashing
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table,
  gridspec-lsl-strings-math]
refs: [server-lsl-value-model, test-lsl-script-corpus]
---

Context: [context/lsl.md](../context/lsl.md).

The first tranche to write, because it is entirely **pure** — no `Host`,
no world, no tick — so it can be implemented and exhaustively tested
before the grid side exists at all, and it is where a precise local
oracle exists.

Roughly 70 functions: `llGetSubString`, `llDeleteSubString`,
`llInsertString`, `llSubStringIndex`, `llStringLength`, `llStringTrim`,
`llToUpper`/`llToLower`, `llGetListLength`, `llList2String`/`Integer`/
`Float`/`Key`/`Vector`/`Rot`/`List`, `llList2ListStrided`,
`llListInsertList`, `llDeleteSubList`, `llListFindList`,
`llListFindStrided`, `llListSort`, `llListSortStrided`,
`llListStatistics`, `llListRandomize`, `llDumpList2String`,
`llParseString2List`, `llParseStringKeepNulls`, `llCSV2List`,
`llList2CSV`, `llStringToBase64`/`llBase64ToString`,
`llXorBase64`, `llIntegerToBase64`/`llBase64ToInteger`,
`llEscapeURL`/`llUnescapeURL`, `llMD5String`, `llSHA1String`,
`llSHA256String`, `llHMAC`, `llChar`/`llOrd`, `llReplaceSubString`,
`llGetSubStringIndex`.

The traps, every one of which is content-visible:

- **Negative and out-of-range indices.** `llGetSubString(s, -5, -1)`
  takes the last five; a start after an end **wraps** (returns the
  outside, not the empty string) — the rule nobody remembers and every
  parser hits;
- `llList2Integer` on a non-numeric entry is `0`, not an error, and the
  whole `llList2*` family coerces rather than failing;
- `llListSort` sorts by the **first element of each stride**, ascending
  or descending, with a stated ordering across mixed types;
- `llParseString2List` versus `llParseStringKeepNulls` — separators and
  spacers are different parameters with different semantics, and the
  empty-token rule is the whole difference between the two;
- `llXorBase64` (and the deprecated `llXorBase64Strings`) has a
  documented cycling-and-truncation behaviour that is nearly impossible
  to guess;
- `llEscapeURL` escapes a specific set, not RFC 3986's.

Oracle: `~/devel/3rdparty/LSL-PyOptimizer/lslopt/lslbasefuncs.py` is a
tested Python implementation of exactly these with SL's edge cases, and
`unit_tests/expr.suite` carries expression cases. Port the table, do not
re-derive it from the wiki.

Acceptance: every function in the tranche implemented (no stubs), each
with table-driven tests whose expectations are taken from the oracle and
whose source is named in a comment; the coverage harness moves the
tranche to fully implemented.

## Measured (gridspec-lsl-runtime-errors, 2026-10-27)

`book/src/gridspec/lsl.md` § Run-time errors, *Indices and strides out of
range*, has what each grid answers for list and string calls past either
end, reversed and with odd strides. Where the flavours differ: Second Life
does not convert a string element for `llList2Vector` / `llList2Rot`
(zero), truncates a float in `llList2Integer` (1.9 → 1) and reads an
exponent in `llList2Float` (`"1e3"` → 1000); OpenSim converts, rounds (2)
and stops at the exponent (1.0). `llDeleteSubList(l, 1, -99)` keeps `[a]`
on Second Life and nothing on OpenSim; `llListFindList` of an empty pattern
is 0 there and -1 here; `llParseString2List` uses only the first eight
separators on Second Life. OpenSim throws (a run-time error on the script,
[[server-lsl-runtime-errors]]) where Second Life answers:
`llList2ListStrided` with a stride of -1 or 99 or a reversed range,
`llListStatistics`' median of strings, `llInsertString` at a negative index.
OpenSim compiles none of `llReplaceSubString`, `llList2ListSlice`,
`llListFindStrided`, `llListSortStrided`; Second Life's answers to an empty
pattern and strides of zero are in the table.

## Measured (gridspec-lsl-strings-math, 2026-10-27)

`book/src/gridspec/lsl.md` § Strings, lists, maths and rotations has the
tables (*Casts and conversions*, *Strings*, *The newer string and list
functions*). What the flavours must differ in:

- **Characters.** Second Life counts code points, OpenSim UTF-16 units: a
  character outside the Basic Multilingual Plane is one there and two here,
  in `llStringLength`, `llGetSubString`, `llSubStringIndex`, `llOrd`,
  `llChar`; OpenSim can hand back half a surrogate pair (`U+FFFD`).
- **Casts.** Second Life wraps `(integer)"2147483648"` (OpenSim −1), skips
  every leading white space (OpenSim only spaces), reads hexadecimal floats
  and `inf` / `nan`, and wants a vector string to start with `<`, taking
  what follows the third component as junk; OpenSim ignores the brackets
  and refuses trailing text. A float is written with 7 significant digits
  by `(string)` but exactly by `llList2CSV` on Second Life; OpenSim writes
  the double. A key's truth is a well-formed key that is not `NULL_KEY`
  there, any non-empty string here.
- **Library.** `llStringTrim` removes only ASCII white space there and
  answers mode 7 as both ends; `llEscapeURL` escapes `~ - _ .` there;
  a bad `%` escape empties `llUnescapeURL`'s result; `llBase64ToString`
  writes `?` for bad UTF-8 and a NUL; `llListSort` of mixed types sorts
  each type among its own places (descending differently on each grid),
  OpenSim turns keys into strings; `llListStatistics` counts integers and
  floats only (OpenSim parses strings and keys); `llListFindList` matches
  a key and a string of the same text there; `llSubStringIndex("", "")` is
  0 there, −1 here; case mapping leaves title case and `İ` alone there.
- **Newer functions**, Second Life only: `llReplaceSubString` replaces
  left to right without overlap, from the right for a negative count;
  `llListFindStrided` matches only at a stride's start counted from
  `start`; `llList2ListSlice` / `llListSortStrided` give `[]` for a stride
  index past the stride; `llComputeHash` and `llHMAC` take the six digest
  names in any case and answer `""` to any other.
- **Cost.** What each call costs as its argument grows is in
  [[server-lsl-call-cost-sizes]].
