---
id: protocol-audit-wire-error-contract
title: sl-wire's public parse surface has five different failure disciplines
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/protocol.md](../context/protocol.md).

Counting the return shapes of `pub fn parse_*` across `sl-wire`: **31**
`Result<_, WireError>`, **14** `Option<_>`, **13** `Result<_, roxmltree::Error>`
(a third-party error type leaked into the public API), **12** `XmlRpcError`,
**4** `LoginParseError`, and **5** that are infallible `Vec<_>` where malformed
input silently yields an empty result (e.g. `parse_render_materials_request`,
`material/legacy.rs:187`).

Same layer, same kind of input, five disciplines. Scope: one `WireError` across
the public surface, wrapping `roxmltree::Error` so callers are not coupled to a
specific XML crate version, and no silent empty-vec results.

Smaller correctness items in the same crate, worth folding into the pass:

- `build.rs:245` — every `Variable`-cardinality block treats a missing count
  byte as an empty block (`reader.u8().unwrap_or(0)`). The comment justifies it
  for OpenSim's shorter `RegionInfo`, but it is emitted for **all** variable
  blocks, so a truncated message decodes "successfully" with silently missing
  data instead of `UnexpectedEof`;
- `build.rs:195-208` — a `Multiple(count)` block's `Vec` length is never
  validated on encode; it writes whatever is there while decode always reads
  exactly `count`, silently producing a malformed packet. The `Variable` arm at
  `:213` does validate;
- `region_handle.rs:99-101` — `from_grid`'s `checked_shl` guards the shift
  *amount*, not value overflow, so the `unwrap_or(0)` is dead and the high bits
  silently truncate for `grid_x >= 2^24`. The existing large-index test
  exercises `from_global`, not `from_grid`;
- `message.rs:44-47` vs `:71` — `MessageId` encode/decode is not injective at
  the boundary: `Low(n)` with `n >= 0xFF00` encodes as `FF FF FF xx` and decodes
  back as `Fixed`. Harmless today (max Low id is `0x1AF`) but unguarded;
- `sl-llsd/src/notation.rs:565` and `value.rs:597` — a malformed base64 body
  silently becomes `Llsd::Binary(vec![])` via `.unwrap_or_default()`, in parsers
  where every other failure returns `MalformedNotation`. Same shape at
  `sl-llsd/src/binary.rs:208`, where an unparsable date silently becomes epoch
  0;
- `sl-llsd/src/notation.rs:181` vs `:343-349` — the crate's two notation walkers
  disagree on map keys: `NotationParser::parse_map` accepts `s(len)"..."` sized
  strings, `Scan::skip_value` does not, so the same document parses with one
  walker and fails with the other;
- `sl-msg-template/src/parser.rs:166` — `Variable N` accepts any `u8`, while
  `sl-wire/build.rs:397` maps `2` to `variable2` and **everything else** to
  `variable1`. Only 1 and 2 occur in the real template, so nothing is broken
  today, but a `Variable 4` would parse cleanly and decode wrongly. The guard
  belongs in `parse_field`. Same file `:81-84`: the trailing-flag loop absorbs
  any word unchecked, where the reference accepts exactly four
  (`llmessagetemplateparser.cpp:519-534`).
