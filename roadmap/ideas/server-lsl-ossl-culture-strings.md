---
id: server-lsl-ossl-culture-strings
title: OSSL's culture-sensitive string search and in-place list sort
topic: server
status: ideas
origin: server-lsl-lib-ossl (2026-11-01)
refs: [server-lsl-lib-ossl, gridspec-lsl-ossl]
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
