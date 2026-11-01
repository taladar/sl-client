---
id: server-lsl-ossl-culture-strings
title: OSSL's culture-sensitive string search and in-place list sort
topic: server
status: done
origin: server-lsl-lib-ossl (2026-11-01)
refs: [server-lsl-lib-ossl, gridspec-lsl-ossl, server-lsl-opensim-shared-lists]
---

Context: [context/lsl.md](../context/lsl.md).

Five OSSL functions [[server-lsl-lib-ossl]] left out because porting them
from `OSSL_Api.cs` needs measurements the probes do not have:

- **`osStringStartsWith`, `osStringEndsWith`, `osStringIndexOf`,
  `osStringLastIndexOf`** compare with .NET's culture-sensitive comparison
  (`StringComparison.CurrentCulture`, `…IgnoreCase`), not unit by unit. The
  strings tranche found one consequence already (the noncharacters
  U+FDD0 … U+FDEF are ignored, `library::text::Text::find_as_grid`); which
  else Mono's collation ignores or folds, and what "ignore case" means
  outside ASCII, is unmeasured. `osStringLastIndexOf(src, value, start,
  count, …)` also calls `LastIndexOf` with a start and a count OpenSim
  computes as for a forward search, which .NET reads backwards from the start
  — so from a start of 0 it throws an `ArgumentOutOfRangeException` on a
  string longer than one character, as far as the source reads. Each of the
  two-overload functions has both overloads to implement.
- **`osListSortInPlace`** sorts the list variable it is given in place, as
  `llSetPayPrice` pads its button list on OpenSim: the compiler writes the
  sorted list back to a variable passed directly (`Instr::PadPayButtons` is
  the pattern). Its order is `LSL_List.Sort`'s.

Wanted: an `ossl/strings` probe on the local OpenSim under both engines —
the four searches over ASCII, case, accents, ligatures, the noncharacters
and a surrogate pair, every overload, and the throwing start — and
`osListSortInPlace` of mixed lists, then the port held to it, and the five
lines of `sl-lsl-runtime/src/library/opensim-scope.txt` moved to `in`.

## Done (2026-11-01)

- **The search is Mono's collator, ported.** OpenSim runs on Mono, so a
  `CurrentCulture` search is `Mono.Globalization.Unicode.SimpleCollator`
  under en-US. `library::collation` ports its search paths line for line
  (`IndexOf`, `LastIndexOf`, `IsPrefix`, `IsSuffix`, `CompareInternal`, the
  ASCII quick path and match cache, kana extenders and special weights),
  reading Mono's own `collation.core.bin` and en-US's lower-case map,
  unassigned code points and 35 contractions, all written by
  `sl-lsl-runtime/mono/dump_collation.cs` off the Mono 6.14.1 the local grid
  runs. The 3000 searches it recorded with .NET's answers are replayed by the
  tests; a 200 000-case set matched too. Only what en-US reaches is ported
  (no multi-unit or sort-key contractions, no diacritic maps, no French
  order); the dumper refuses a culture that has them.
- **All five functions in** (`opensim-scope.txt`: 60 in, 166 later, 10 out):
  the four searches with both overloads, `osStringLastIndexOf`'s start read
  as .NET's end and its `ArgumentOutOfRangeException` (`Count must be
  positive …`, YEngine telling the owner a line at a time and stopping the
  script, XEngine shouting it), and `osListSortInPlace` through a new
  `Instr::CallBuiltinWriteBack` that stores the sorted list back into the
  list variable passed (`ScriptCtx::write_back`).
- **Beyond the plan:** `llSubStringIndex` and OpenSim's
  `llParseString2List` / `llParseStringKeepNulls` used an approximation of
  the same search (`Text::find_as_grid`, which only ignored U+FDD0 …
  U+FDEF); they now use the collator, and the parse loop is OpenSim's own
  (a delimiter found nowhere is dropped, a match at the very end is none,
  and a search that would start past the end throws `Index was out of
  range …`, as an expansion matched at the end makes it do).
- **Measured**: `ossl/strings` (a `Strings` script, a plain-LSL `Parser` and
  a leader) on both OpenSim engines and aditi (where `Strings` does not
  compile), held by `strings_search_and_sort_as_each_engine_did` line for
  line on both engines and every exception line.
- **One named deviation, not modelled**: OpenSim's lists are shared objects,
  so an in-place sort shows through every variable holding the list (a copy,
  the original of a sorted copy, the caller's variable after a function
  sorted its parameter); here only the variable passed changes. Handed to
  [[server-lsl-opensim-shared-lists]], with `llSetPayPrice`'s padding, which
  has the same deviation.
- **Not probed**: a lone surrogate (half a pair), which the value model's
  strings cannot hold.
