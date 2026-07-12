---
id: protocol-55
title: ExtraParams encoder (extends #16, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**55. `ExtraParams` encoder (extends #16, Tier C). ✅ Done.** `extra_params.rs`
had `decode_extra_params` only. Added the inverse `encode_extra_params`: the
container framing (a `u8` count of present parameters, then per-param
little-endian `u16` type / `u32` size / payload) plus a faithful port of each
subtype's `LLNetworkData::pack` from `indra/llprimitive/llprimitive.cpp` —
flex/light/sculpt/light-image/extended-mesh/render-material/reflection-probe. A
field that is `None` (or, for render materials, an empty list) is omitted, so a
default round-trips to a lone zero count byte; present parameters are emitted in
ascending type-code order (the order the viewer's parameter list, keyed by
`type >> 4`, iterates). Subtype details mirror the decoder's inverses: the
flexi "softness" bits are re-stashed in the high bits of the tension/drag bytes
with the viewer's `* 10.01`-then-truncate quantization; the reflection-probe
booleans are recombined into the box/dynamic/mirror flag byte; render materials
are capped at the viewer's 14-entry block limit; and sculpt/mesh is always
written under the canonical `PARAMS_SCULPT` code (the decoder accepts the
`PARAMS_MESH` alias too). Exported from `sl-proto` alongside
`encode_texture_entry`; no runtime wiring (a server-side binary sub-codec,
reused by #57's `ObjectUpdate` body assembly and #60's `SimSession`). Covered by
three new `extra_params.rs` tests (default → lone zero byte; a fully-populated
`encode`→`decode` round trip across all seven subtypes that also checks the
encode is the exact deterministic inverse; and the 14-entry render-material
cap). *Test: unit round-trip (no grid).*
