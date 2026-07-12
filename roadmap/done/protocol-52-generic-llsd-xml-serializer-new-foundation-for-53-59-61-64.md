---
id: protocol-52
title: Generic LLSD-XML serializer (new, foundation for #53/#59/#61–#64)
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**52. Generic LLSD-XML serializer (new, foundation for #53/#59/#61–#64). ✅
Done.** `sl-wire/src/llsd.rs` parsed LLSD-XML into an [`Llsd`] tree but could
only *serialize* via the bespoke per-request string builders. Added
`Llsd::to_llsd_xml`, which emits any tree as a complete `<llsd>…</llsd>`
document — the element-by-element inverse of `parse_llsd_xml`/`node_to_llsd`:
`<undef />`, `<boolean>true|false</boolean>` (round-trips through the parser's
`1`/`true` acceptance), `<integer>`/`<real>` (Rust's shortest finite-float
formatting), `<uuid>`, `<string>`/`<date>`/`<uri>` (all run through the existing
`push_escaped`), `<binary>` (standard base64, the inverse of the parser's
decode), and recursive `<array>`/`<map>`. Map keys are emitted in **sorted**
order so two equal `Llsd` trees serialize byte-for-byte identically (LLSD maps
are unordered, so the order is a free choice made deterministic). This is the
foundation every CAPS- and login-side LLSD producer (#53/#59/#61–#64) builds on
rather than hand-concatenating XML. Covered by four `sl-wire/tests/llsd.rs`
round-trip tests: every scalar kind (incl. XML-metacharacter escaping) →
serialize → re-parse-equal, nested arrays/maps round-trip, deterministic
sorted-key output (exact-string assertion), and a hand-built `EventQueueGet`
response that the existing `parse_event_queue_response` reads back. *Test: unit
round-trip (no grid).*
