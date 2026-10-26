---
id: gridspec-lsl-json-linkset-data
title: LSL library behaviour on each grid — llJson* and the linkset data store
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-json-linkset-data]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Linkset data budget and accounting, `linkset_data` event shapes
(aditi only). Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Lljson* and the linkset data store.

## Fake grid

Large — [[server-lsl-lib-json-linkset-data]].

## Viewer

None.

## Done (2026-10-26)

- **Discover.** Two probes, `sl-lsl-runtime/probes/data/`: `json`
  (`llList2Json` of every LSL type and of strings that look like JSON,
  numbers, escapes and non-ASCII text; `llJson2List` of valid and broken
  JSON; `llJsonGetValue`, `llJsonValueType` and `llJsonSetValue` along
  paths that exist, run past an array, mix keys and indices or are empty;
  how the string functions treat a sentinel; whether the functions change
  the list they are given) and `linkset-data` (three scripts: the budget,
  what each call costs and answers, protected pairs, listing, finding and
  deleting by pattern, filling the store, `llResetScript`, another object's
  store, and the `linkset_data` event as the writer and a second script in
  the prim see it). Complete on aditi and both OpenSim engines, which agree
  on every line. Second Life's JSON keeps the text as written, is lenient,
  does not decode escapes in `llJson2List` and writes nothing for
  `JSON_INVALID`; OpenSim writes everything anew, parses only a document
  starting with `[` or `{`, has `JSON_APPEND` as a string, adds `0` to an
  empty specifier list it is given, and its string functions treat the
  sentinels as ignorable. OpenSim has no linkset data: the probe does not
  compile there, which is its measurement.
- **Library table.** The probes need `llLinksetData*`, which the vendored
  document lacked, so [[server-lsl-library-table-refresh]] was done first:
  `sl-conformance` now keeps the `LSLSyntax` document a grid serves, and the
  one aditi served replaced the viewer's copy.
- **A wedged engine.** The first `data/json` replaced each sentinel in a
  value with `llParseStringKeepNulls`; on OpenSim, where a sentinel is found
  between every character, that grew the string tenfold per sentinel and
  held a YEngine thread until OpenSim was restarted (the prims had to be
  deleted from `OpenSim.db`, since `delete object` waits on the thread). The
  probe now compares character by character, and `book/src/tools/lsl-probes.md`
  says so.
- **Not measured.** Linkset data travelling with a take and a re-rez, a
  linked child prim's script (the probe's prims are linked to nothing), and
  the `linkset_data` event across a region restart; `llJson*` on documents
  near the 64 KiB script memory.
- **Document.** `book/src/gridspec/lsl.md` § llJson* and the linkset data
  store; two notes in `book/src/tools/lsl-probes.md` (Second Life evaluates
  right to left; never look for a sentinel with a string function on
  OpenSim).
- **Fake grid.** Large, as planned: the rows are in
  [[server-lsl-lib-json-linkset-data]].
- **Viewer.** None.
