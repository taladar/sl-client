---
id: protocol-audit-notecard-fidelity
title: The notecard codec misreads metadata, corrupts on re-encode, and misplaces embedded items
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-asset-decoder-allocation-caps]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-notecard` diverges from the reference in five places, and the module doc
(`encode.rs:5-7`) claims a live-grid notecard "re-encodes byte-for-byte" — which
fails for all of them.

- **`decode.rs:341` — the `metadata` field spans two lines on the wire.**
  Firestorm writes `"\t\tmetadata\t"` + `LLSDSerialize::toXML(...)` (which
  always ends `"</llsd>\n"`, `llsdserialize_xml.cpp:73`) + `"|\n"` on the
  **next** line (`llinventory.cpp:963-971`). Decode captures line 1 as
  `metadata` and line 2 (`"|"`) falls through to `unknown_fields` (`:356`);
  encode then emits a one-line form **plus a bogus `\t\t|` line**
  (`encode.rs:70-72`). The doc at `item.rs:113-115` describes a form the
  simulator never writes.
- **`encode.rs:67` — `name` / `desc` are written unescaped.** With
  `name = "a|b"`, `tabbed_value` (`decode.rs:238`) splits on the first `|` and
  returns `"a"`. Worse, a `\n` in `name` / `desc` / `metadata` writes extra
  lines into the item chunk, letting an inventory-item name **rewrite
  `asset_id` or `permissions` on save**. The reference sanitizes on import
  (`llinventory.cpp:884`); this crate sanitizes on neither side.
- **`lib.rs:111` — embedded items are resolved by the stored `ext char index`,
  which the reference ignores.** `llnotecard.cpp:99-101` reads `index` into a
  local it never uses; `LLEmbeddedItems::addItems`
  (`llviewertexteditor.cpp:660-676`) assigns `FIRST_EMBEDDED_CHAR + 0,1,2...` by
  **stream position**. So a notecard whose sole item carries `ext char index 1`
  renders the item in Firestorm and nothing here. `encode.rs:99` also re-emits
  `char_index` verbatim where the reference rewrites it to the position
  (`llnotecard.cpp:247`), so output re-indexed by the reference silently
  reattaches items to different markers.
- **`decode.rs:339` — brace-shaped unknown fields desynchronise the item
  parser.** `parse_permissions` / `parse_sale_info` (`:249-254`) swallow a
  leading `{` if present but tolerate its absence and stop at the first `}`. An
  unrecognised chunk-shaped field leaves its `{` as noise and its `}` closes the
  *item*; `permissions 0` with no `{` makes the parser eat the item's own
  closing brace. No panic, but every following item misparses.
- **`decode.rs:384` — `LLEmbeddedItems version` is accepted for any `u32`, and
  three sites disagree.** `llnotecard.cpp:64-68` fails the import unless the
  version is 1. Here decode accepts anything, `encode.rs:90-94` echoes it back
  (the reference hardcodes 1), and `edit.rs:72` applies a third rule
  (`.max(1)`).

`sl-notecard` also has **no `tests/` directory at all** — unique among the
sibling parser crates. Coverage is 16 tests in `lib.rs` plus 7 in `edit.rs`,
with two negative cases; nothing exercises a hostile `count`, `shadow_id`, a
missing brace or non-UTF-8 lines, and `types.rs`'s ~70-arm `from_type_name` /
`type_name` inverse pair is entirely untested.
