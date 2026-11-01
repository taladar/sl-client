# Mono's en-US collation, vendored

OpenSim runs on Mono, and a culture-sensitive string search there — every
`String.IndexOf(string)` with no comparison named and every
`StringComparison.CurrentCulture…` search — goes through Mono's managed
collator (`Mono.Globalization.Unicode.SimpleCollator`) under the en-US
culture OpenSim sets on its threads. `llSubStringIndex`, the delimiters of
`llParseString2List` and `llParseStringKeepNulls`, and the OSSL searches
`osStringIndexOf`, `osStringLastIndexOf`, `osStringStartsWith` and
`osStringEndsWith` are such searches. The script engine ports the collator
(`../src/library/collation.rs`); this directory holds what the port reads
and what it is held to.

All three data files were written by `dump_collation.cs` with the Mono the
local OpenSim runs, 6.14.1:

```sh
mcs -nologo -out:/tmp/dump_collation.exe dump_collation.cs
mono /tmp/dump_collation.exe tables .
mono /tmp/dump_collation.exe cases collation-cases.txt 20261101 3000
```

- `collation.core.bin` is the `collation.core.bin` resource of Mono's
  `mscorlib.dll`, copied out byte for byte (SHA-256
  `418209a0f7e0380c6c96279fdcc7f293103a759f8da72715b41a7dfff97d7d12`): the
  ignorable flags, categories and three levels of weights every culture's
  collator reads (`MSCompatUnicodeTable`).
- `culture-en-us.txt` is what en-US adds, read off the running Mono: the
  lower-case map of its `TextInfo`, the code units .NET counts unassigned (which
  the collator ignores), and the collator's contractions (en-US has the
  invariant culture's: 35 units each read as their replacement). The dumper
  refuses to write it if en-US ever gains a multi-unit contraction, a
  contraction to a sort key, a diacritic remapping or French accent order,
  none of which the port has.
- `collation-cases.txt` is 3000 random searches over the units the collator
  treats specially, with every answer .NET gave: `IndexOf`, `LastIndexOf`,
  `StartsWith`, `EndsWith`, each with and without a range, case-sensitive and
  ignoring case. The port's tests replay all of them; the environment
  variable `SL_LSL_COLLATION_CASES` points the same test at a larger set
  written the same way (200 000 cases with seed 7 matched when the port was
  written).

Mono's class libraries, the collator and the tables generated for it among
them, are under the MIT licence; `LICENSE-Mono.txt` is its notice as the
collator's sources carry it.
