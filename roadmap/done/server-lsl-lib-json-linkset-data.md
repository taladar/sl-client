---
id: server-lsl-lib-json-linkset-data
title: Library tranche — llJson* and the linkset data store
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table,
  gridspec-lsl-json-linkset-data]
refs: [server-lsl-lib-strings-lists, server-world-link-sets]
---

Context: [context/lsl.md](../context/lsl.md).

Two small, modern surfaces that pair naturally: JSON is how scripts
serialise, and linkset data is where they put the result.

**`llJson*`** — `llJsonGetValue`, `llJsonSetValue`,
`llJsonValueType`, `llList2Json`, `llJson2List`, and the `JSON_*`
constants. The trap is that this is **not** ordinary JSON handling:

- values come back as *strings* with LSL's own sentinels
  (`JSON_INVALID`, `JSON_NULL`, `JSON_TRUE`, `JSON_FALSE`, each a
  specific unlikely character), so a caller distinguishes "absent" from
  "the string that looks like absent" only by convention;
- the specifier list addresses nested objects and arrays by alternating
  key and index, with `JSON_APPEND` for "past the end";
- number formatting on the way out, and what counts as a valid number on
  the way in, follow Linden's parser, not `serde_json`'s;
- `llList2Json` type-infers each element, and round-tripping a list
  through JSON is lossy in ways content works around.

`~/devel/3rdparty/LSL-PyOptimizer/lslopt/lsljson.py` (687 lines) is a
deliberate reimplementation of Linden's dialect, written because the
standard parsers disagree with it. Port it rather than reaching for
`serde_json`.

**`llLinksetData*`** — `llLinksetDataWrite`, `Read`, `Delete`,
`Reset`, `CountKeys`, `ListKeys`, `FindKeys`, `Available`,
`CountFound`, and the protected-by-password variants
(`llLinksetDataWriteProtected` and friends), plus the `linkset_data`
event raised on every change. It is a key/value store **owned by the
linkset** ([[server-world-link-sets]]), not by a script: it survives a
script reset, is shared by every script in the set, travels with a
take/rez, and has a byte budget (~128 KB) that `Available` reports.
Second Life only; OpenSim has no equivalent, so the local grid is not an
oracle here and the SL wiki plus a script run on aditi is.

Acceptance: both families implemented; a JSON round-trip corpus ported
from `lsljson.py`'s own cases; linkset data shown to survive a
`llResetScript`, to be visible from a second script in the same linkset
and invisible from a different object, and to raise `linkset_data` with
the right action.

## Measured (gridspec-lsl-json-linkset-data, 2026-10-26)

The rows are in `book/src/gridspec/lsl.md` § llJson* and the linkset data
store, from the `data/*` probes on aditi and both OpenSim engines; the fake
grid should take each flavour's column. The parts that shape the
implementation:

- **Second Life keeps the text.** `llJsonGetValue` and `llJsonSetValue`
  answer numbers and untouched values as written (`2.50`, `-1.0e2`,
  `[ 1 ]`); OpenSim writes everything anew, compact (`2.5`, `-100.0`). A
  port of `lsljson.py` keeps the text, which is Second Life's flavour.
- **Second Life's `llJson2List` is lenient and does not decode escapes**:
  it parses a scalar and whitespace round a document, reads `[1,]`, `[1.]`,
  `[.5]`, `["a" "b"]` and `[truth]`, clamps an integer to 2147483647, and
  returns a string with escapes from its opening quote to the end of the
  text. OpenSim parses only a document starting with `[` or `{`, decodes
  escapes, accepts single quotes, and answers anything else with the whole
  text.
- **`JSON_APPEND` is the integer -1 on Second Life**, the string `"-1"` on
  OpenSim; a string index (`"0"`) reads an array element on Second Life
  only.
- **`JSON_INVALID` as a value writes nothing** on Second Life (`[,…]`,
  `{"b":}`); `llList2Json` writes infinity as `inf`.
- **OpenSim's own bugs, for its flavour or not**: it adds `0` to the
  specifier list a script gives it when that list is empty, and its string
  functions treat the sentinels as ignorable (`llSubStringIndex` finds
  `JSON_INVALID` at 0 in any string). Whether the OpenSim flavour imitates
  these is the flavour's call; the second is a .NET culture-sensitive
  comparison rather than anything LSL says.
- **Linkset data is Second Life's only**: OpenSim has neither the functions
  nor the event, so its flavour refuses to compile them. The budget is
  131072 bytes; a pair costs its name and value in UTF-8, a protected one
  32 bytes more; the return codes, the event's four actions (one
  `LINKSETDATA_MULTIDELETE` per `llLinksetDataDeleteFound` call, names
  comma-separated; a protected pair's value hidden) and the key order
  (code points) are in the table. The store survives `llResetScript`, is
  shared by every script in the linkset and invisible to another object.

## Done (2026-10-31)

All 18 functions are implemented in `sl-lsl-runtime`, each answering as the
region's flavour does; coverage is 130 implemented of 524.

**JSON** (`library/json_text.rs`, dispatching per flavour).

- **Second Life** (`json_text/second_life.rs`) is PyOptimizer's `lsljson.py`
  scanner and `llJsonGetValue` walk, adjusted to aditi: a string ends at the
  first `"` not preceded by `\` (which is why `["A\n\/\"\\"]` reads as one
  string to the end of the text — escapes *are* decoded, only the one-letter
  ones, any other escaped character being itself; the measured note above
  that it "does not decode escapes" was a misreading, corrected in the book),
  a number may not start with a needless zero, a word ends at `:` too, and
  `llJson2List` is lenient in an array (the end of the text closes it, a value
  without a comma replaces the one before). `llJsonSetValue`, which
  PyOptimizer never implemented, is written from the probe's rows and LL's
  own test script: the levels the path goes through are rewritten compact
  around members kept as written, what is not a container where the path
  goes on is replaced by what the path makes. LL's test script (PyOptimizer's
  `unit_tests/json.py`) is ported whole as unit tests; its one disagreement,
  `1.0e+1` as a number, is named (today's grid takes no `+` exponent, per
  PyOptimizer).
- **OpenSim** (`json_text/opensim.rs`) ports `LSL_Api.cs` and the LitJson of
  OpenSim's libopenmetaverse fork (now cloned at
  `~/devel/3rdparty/libopenmetaverse-opensim`), its corners checked against
  OpenSim's own `LSL_Api` run under Mono with the shipped DLLs; the book's
  engine chapter lists them.
- **The dialect**: OpenSim's `JSON_APPEND` is the string `"-1"`
  (`Dialect::constant_override`), and OpenSim compiles none of the linkset
  data functions, constants or the `linkset_data` event.
- **OpenSim's string search ignores the sentinels** (`Text::find_as_grid`,
  .NET's culture-sensitive `IndexOf` for U+FDD0 … U+FDEF), so
  `llSubStringIndex` and the parsers answer `data/json`'s `sentinel-search`
  rows as OpenSim does.

**Linkset data** (`vm/linkset_data.rs`, `library/linkset.rs`). The store is a
plain value the host keeps per linkset (`Host::linkset`,
`Host::linkset_data`); one password comparison explains every protected row.
A change raises `linkset_data` in every script of the linkset once the slice
is over, the writer's own dropped when the slice ends in a reset — on aditi
the event is queued at the call, so the writer's queue held 63 events and its
own link message, and a reset in the same event lost the writer's.

**Proof.** `vm/tests/json.rs` replays `data/json` on all three grids. Named
OpenSim deviations: the value model's floats (`list2json floats`), a lone or
half surrogate a Rust string cannot hold (`lone-surrogate`, and the probe's
own per-character print of an emoji in `list2json strings` and
`json2list surrogates`), and `shared-list` — OpenSim adds `0` to the caller's
own empty specifier list, a .NET reference there; lists are values in this VM,
so that is not imitated. `vm/tests/linkset.rs` runs the probe's three scripts
in two linksets, with the test carrying their link messages and chat (no comms
library yet): every Leader and Stranger record matches aditi but
`fill-events` (64 here, because no link message reaches the Leader's queue),
and the Watcher's 25 events before the fill match; after it, what the Watcher
sees is timing (`vary: got`). A second test holds the event's reach and the
reset rule.

**Not done**: the store travelling with a take and a re-rez is the host's
(the fake grid's wiring, [[server-fake-grid-script-engine-wiring]]).
