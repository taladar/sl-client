---
id: protocol-audit-decoder-fuzz-harness
title: Fuzz the wire and asset decoders
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [protocol-audit-llsd-recursion-depth-cap, protocol-audit-mesh-decode-allocation-caps, protocol-audit-asset-decoder-allocation-caps]
---

Context: [context/protocol.md](../context/protocol.md).

There is **no fuzz or property-test harness anywhere in the workspace**: no
`fuzz` directory, and no `proptest` / `arbitrary` / `quickcheck` in any
manifest. Every parser in `sl-wire`, `sl-llsd`, `sl-notecard`, `sl-mesh`,
`sl-anim` and `sl-texture` consumes unauthenticated network bytes, and this is
the structural gap that let the recursion and allocation findings exist.

Three targets, in order of value:

- the three LLSD entry points (`parse_llsd_binary` / `_notation` / `_xml`);
- `Notecard::decode`;
- `AnyMessage::decode` behind `parse_datagram` + `zero_decode`.

The invariant is the same for all three: arbitrary bytes must terminate, must
not panic, and must not allocate more than a bounded multiple of the input.

Cheap companion, not fuzzing but the same spirit: `sl-wire`'s generated codec
covers **6 of 483 messages** in `tests/messages_roundtrip.rs`. A single
template-driven `for each message: encode(default) -> decode -> assert_eq` loop
covers all 483 for a few lines, and would exercise `LLVector4`, which appears
exactly once in the template and is never tested today.
