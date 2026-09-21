---
id: server-lsl-lib-strings-lists
title: Library tranche — strings, lists, encoding and hashing
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table]
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
