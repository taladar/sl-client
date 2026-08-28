---
id: protocol-audit-notecard-fidelity
title: The notecard codec misreads metadata, corrupts on re-encode, and misplaces embedded items
topic: protocol
status: done
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

## Fixed (2026-08-28)

All five, plus the missing `tests/` directory. The two structural ones changed
the crate's model rather than patching a call site.

**Embedded items are positional.** `EmbeddedItem` is gone; `Notecard::items` is
a `Vec<InventoryItem>` whose order *is* the numbering the text's markers
resolve against, and `item_by_index` is `items.get(index)`. That is what the
reference does — `llnotecard.cpp` reads `ext char index` into a local it never
uses and `LLEmbeddedItems::addItems` numbers by load order — so keeping a
`char_index` field could only ever disagree with the answer Firestorm gives.
`encode` writes the position, as `exportEmbeddedItemsStream` does, instead of
echoing what it read.

**The chunk version is a constant.** `EMBEDDED_ITEMS_VERSION = 1`. Decode
rejects anything else (the reference fails the import), encode writes 1, and
`Notecard::embedded_items_version` — the field the three sites disagreed about
— no longer exists, which removes `edit.rs`'s third rule with it.

**`metadata` spans two lines.** Decode takes the whole value after the tab (the
reference's `%254s`, which does *not* stop at a `|`) and consumes the lone `|`
terminator line, so it stops falling through to `unknown_fields`; encode writes
the two-line form. A one-line variant with a trailing `|` is still accepted and
normalised. A notecard carrying a thumbnail now re-encodes byte-for-byte, which
is what the module doc always claimed.

**Free text cannot reshape the stream.** `name` / `desc` are written through a
sanitiser that replaces `|` and line breaks with a space (the reference does
the same on import); `metadata` and preserved unknown lines get the line-break
half, since a `|` there is content. A `\n` in an item name could otherwise
write extra lines into the item chunk and rewrite `asset_id` or the whole
`permissions` block on save.

**Chunk framing is required, not inferred.** `parse_item` /
`parse_permissions` / `parse_sale_info` now consume their opening `{`
explicitly, and a `{` appearing where a field belongs is an error. The
reference `continue`s past it and lets the nested `}` close the enclosing
chunk, silently misparsing every following item; there is no recovering the
framing once that happens, so a decoder that returns a `Result` says so. This
is the one deliberate divergence from the reference, and it is strictly in the
direction of not handing back mangled inventory.

`sl-notecard/tests/notecard.rs` is the crate's first integration test file: 20
cases over the simulator's real shapes and the hostile ones — the two-line
metadata round-trip, a name that tries to forge an `asset_id`, a lying `ext
char index`, a nested unknown chunk, a `permissions` line with no body, a
non-1 chunk version, a malformed `shadow_id`, an unterminated item, a non-UTF-8
container line, an empty stream, and the `from_type_name` / `type_name` inverse
pair for all three ~30-arm tables.
