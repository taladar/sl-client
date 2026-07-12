---
id: protocol-54
title: TextureEntry encoder (extends #16/#20, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**54. `TextureEntry` encoder (extends #16/#20, Tier C). ✅ Done.**
`sl-proto/src/appearance.rs` had `decode_texture_entry` only. Added the inverse
`encode_texture_entry`, a faithful port of the reference viewer's
`LLPrimitive::packTEMessage`/`packTEField`: the **last** face's value becomes
each field's default, then faces are scanned high→low and every value not
already carried by a higher-indexed face is emitted as a `(face bitmask, value)`
override (the bitmask flagging all at-or-below faces that share it), with the
per-field terminating zero bitmask written between the eleven fields — and none
after the trailing material field, which the decoder self-terminates. The
natural-unit values are re-quantized to the wire grid (colour re-inverted to
`255 − channel`; offsets `clamp(−1,1)·0x7FFF`; rotation `fmod(·,2π)/2π·0x8000`;
glow `clamp(0,1)·0xFF`) — the exact inverses of the decoder's de-quantization —
and faces beyond [`MAX_FACES`] (64, the wire bitmask width) are dropped to match
the decoder's cap. The variable-length face bitmask is emitted as the
most-significant-first base-128 integer the decoder reassembles. Exported from
`sl-proto` alongside `decode_texture_entry`; no runtime wiring (a server-side
binary sub-codec, reused by #57's `ObjectUpdate` body assembly and #60's
`SimSession`). Covered by three new `appearance.rs` tests (empty entry → empty
blob; an `encode`→`decode` round trip over exactly-representable values with a
shared run that exercises the default-plus-override packing and colour
re-inversion; and `decode`→`encode`→`decode` idempotency over a hand-built blob
with non-trivial quantized offset/rotation/glow and a multi-face override).
*Test: unit round-trip (no grid).*
