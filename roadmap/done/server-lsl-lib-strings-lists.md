---
id: server-lsl-lib-strings-lists
title: Library tranche — strings, lists, encoding and hashing
topic: server
status: done
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
[[server-lsl-runtime-errors]]) where Second Life answers: `llList2ListStrided`
of a one-element range past the list, `llListStatistics`' median of strings,
`llInsertString` at an index past the end. (This read "a stride of -1 or 99
or a reversed range" and "at a negative index" until the replays below
showed which call of each line threw; the book is corrected.)
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
- **Cost.** Each function whose work grows with its argument is charged
  its measured cost on top of the flat call: the per-size times are in book
  `gridspec/lsl.md` § *What a call costs* (aditi transcripts
  `probes/results/aditi/vm/call-cost-*.txt`), and `sl-lsl-runtime`'s
  `vm/cost.rs` has the shape to put them in — a `SizeCost` of measured
  points per function, as `LIST_JOIN` and `STRING_JOIN` are for `+`
  ([[server-lsl-call-cost-sizes]]). On Second Life every string function
  grows with the string, `llStringLength` and `llGetSubString` included.
  Extend `joins_and_flat_calls_cost_what_they_cost_on_aditi` (in
  `vm/tests.rs`) to each function as it lands.

## Done (2026-10-30)

All 52 functions of the tranche (51 new beside `llGetListLength`) are
implemented in `sl-lsl-runtime` (`library/strings.rs`, `lists.rs`,
`encoding.rs`), each answering as the
region's flavour does where the grids were measured to part: Second Life's
from PyOptimizer, OpenSim's ported from `LSL_Api.cs` / `LSL_Types.list`
(including its throws, its statistics — the range is max ÷ min there — its
pivot sort and its `UUID.TryParse` view of strings and keys). Coverage is 83
implemented of 524. Supporting pieces: a `PureCtx` trait the pure functions
are generic over (flavour, a refusal, OpenSim's second of sleep after one);
`text.rs`, which counts in code points or UTF-16 units by flavour;
`range.rs`, both grids' index arithmetic; `CallError::Exception` /
`RuntimeError::Exception`, which YEngine tells the owner and XEngine says on
`DEBUG_CHANNEL` (`OverflowException` too, for OpenSim's `llList2Integer` of a
float past the range); `Host::random_bits` and `ScriptCtx::random_below` for
`llListRandomize`'s seeded shuffle; and `vm::cost::call`, which charges each
call its measured cost at its argument's size.

**Proof.** `vm/tests/strings.rs` runs the probes' own scripts in the VM
(`llOwnerSay` as `print`, the region queries written in) and holds every line
they print to the transcripts: `strings/functions`, `strings/conversions`,
`strings/unicode` and `strings/newer` on aditi; `strings/functions` and
`strings/unicode` on YEngine and XEngine; all 18 `errors/bounds` cases on the
three and the 4 `errors/bounds-newer` cases on aditi, with the owner lines
YEngine tells and the debug line XEngine says for the three that throw. The
named deviations: `llBase64ToInteger("!!!!")`, a different number each run
on aditi, is 0 here; and bounds cases 1–3 on OpenSim print a vector with
`(string)`, which writes five decimals here (the value model is Second
Life's) and six there. `joins_and_library_calls_cost_what_they_cost_on_aditi`
holds 121 of the 124 timed cells within 5 % — the three left out are the
committed runs' outliers (book ranges 4.2–7.9, 113–639, 63–661 µs) — and
`math/float-rounding`'s `list-element` and `list-append` rows are now in the
precision replay.

The replays found three answers the oracles had wrong:
`llList2ListSlice` of a reversed range strides each of its two pieces from
its own start (`[1, 2, c, d]`, not `[1, 2, 4]`); `llListFindStrided` with a
stride of 0 searches as 1 (PyOptimizer: −1); and OpenSim's
`Uri.UnescapeDataString` writes a kept bad escape in upper case. They also
corrected five book rows (OpenSim's throws above, `llListFindList` of a key
against a string, which only OpenSim matches, and the odd-length
`llListSortStrided`, which is left as it is).

**Decisions beyond the measurements.** A function the cost probes did not
time is charged as the timed one doing the same work (`llToLower` as
`llToUpper`, the other single reads as `llList2Integer`, `llHMAC` as
`llComputeHash`, …; the list is in `vm::cost::library_cost`). The deprecated
`llXorBase64Strings` / `llXorBase64StringsCorrect` and the unmeasured
`llListFindListNext` follow PyOptimizer on every grid. `llLinear2sRGB` and
`llsRGB2Linear` (maths, but measured by `strings/newer`) are implemented too
so the probe replays whole. OpenSim's `llListSort` of a list of one type
uses a stable sort where .NET's `Array.Sort` is not; equal keys can only
differ in order there.
