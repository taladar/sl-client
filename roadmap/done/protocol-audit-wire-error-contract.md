---
id: protocol-audit-wire-error-contract
title: sl-wire's public parse surface has five different failure disciplines
topic: protocol
status: done
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

## Done (2026-09-21)

**One `WireError`.** `XmlRpcError` and `LoginParseError` are gone; their field
faults fold onto the `LlsdError` and `InvalidScalar` variants that already said
the same thing, and `WireError` grew the XML-RPC family (`NotXmlRpc`,
`NoMethodName`, `UnexpectedMethod`, `XmlRpcFault`, `NoStruct`),
`MalformedCompressedBody`, `BlockCountMismatch`, and two XML variants. The XML
wrap carries the reader's diagnostic as *text*, not its type, so no `roxmltree`
name crosses the boundary; `XmlNestingTooDeep` keeps the one distinction worth
matching on (refused for depth, not malformed). Two login parsers stopped
swallowing the XML error entirely — they mapped it to "no response struct".

**`sl-wire/tests/parse_error_contract.rs` pins it.** It reads the crate's own
sources: every `pub fn parse_*` must return `Result<_, WireError>` except the
fourteen **route matchers** pinned by name with the route each matches — a URL
suffix or tile file name answering "not mine", which is a decision, not a
failure. Checked both ways, plus a third test pinning the scanner so a
regression in it cannot make the other two pass by finding nothing.

**Resolved differently from the audit's wording, with reasons.**

- `region_handle.rs` was already fixed in `3422d59a` (`saturating_mul`); only
  the missing `from_grid` large-index test was owed, and is here.
- The `Variable` count tolerance is narrowed to a message's **trailing run** of
  `Variable` blocks, not just its last block: OpenSim's shorter `RegionInfo`
  drops three at once. Five blocks in the whole template are `Variable`
  followed by a non-`Variable` one, and those read the count strictly.
- `binary.rs`'s epoch fallback for an unparsable date is **still there, and now
  unreachable from any document**. The fix went to the readers instead: both
  textual readers admit a `Date` only when it is a timestamp the binary form
  can carry (`representable_date`) — XML maps the rest to `Undef`, notation
  refuses them — so nothing off the wire becomes a bogus 1970. Making
  `to_llsd_binary` fallible was tried and reverted: it puts a `Result` on ~30
  call sites, fourteen of them `RenderMaterials` bodies and mesh headers whose
  types cannot hold a date at all, and several (`CapsResponse::llsd_xml(...)`,
  the fire-and-forget runtime PUTs) cannot propagate one — their only option
  would be the silent `unwrap_or_default` this pass exists to remove.
- The XML walk's undecodable `<binary>` became `Undef` rather than an error:
  that walk is lenient by design and documented as such, with no error channel.
  `Undef` reads as *absent* instead of as a present zero-length payload, which
  is a difference the caller can act on. Turning the whole walk strict is its
  own task, and carries live-grid risk this one should not.

**Found on the way, and not in the audit:** `parse_binary` dispatched on the
*first digit* of the notation radix marker, and `b'1' | b'6'` catches `b16` and
`b64` alike — so every `b64` body went to the hex decoder and decoded to
garbage or, for most payloads, nothing. The marker is read whole now. Both
base64 readers also strip ASCII whitespace throughout: the decoder refuses an
embedded newline, so a line-wrapped body previously decoded to nothing.

Commits: `5c0fa644` (one `WireError`), `71f4a07a` (the three codec coercions),
`fa7d6f06` (the LLSD readers and the template parser), and this one.
