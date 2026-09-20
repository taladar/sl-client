---
id: server-lsl-lib-json-linkset-data
title: Library tranche — llJson* and the linkset data store
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-library-surface-table]
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
