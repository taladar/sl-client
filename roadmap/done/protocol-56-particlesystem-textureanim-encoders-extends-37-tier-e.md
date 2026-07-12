---
id: protocol-56
title: ParticleSystem + TextureAnim encoders (extends #37, Tier E)
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**56. `ParticleSystem` + `TextureAnim` encoders (extends #37, Tier E). ✅
Done.** `particles.rs` decodes the legacy 86-byte and modern size-prefixed
particle forms and the 16-byte texture-anim block. Added the inverse
`encode_texture_anim` and `encode_particle_system`. `encode_texture_anim` is a
port of the viewer's `LLTextureAnim::packTAMessage` — the four header bytes
(mode / face / grid x / grid y) then three little-endian `F32`s — writing the
grid dimensions verbatim (the decoder, not the encoder, applies the
non-`SMOOTH` floor-to-1). `encode_particle_system` chooses the wire form the way
the decoder distinguishes them (mirroring `LLPartSysData::isLegacyCompatible`):
a system carrying neither glow nor blend-func data (neither `LL_PART_DATA_GLOW`
nor `LL_PART_DATA_BLEND` in `part_flags`) is the legacy fixed 86-byte form — the
68-byte source block then the 18-byte legacy particle block, no size prefixes —
otherwise the modern form, prefixing each sub-block with its `S32` size and
appending the glow / blend-func bytes gated by those flags. Every fixed-point
field is re-quantized with the exact inverse of the decoder's `unpackFixed`
(`LLDataPacker::packFixed`): clamp to range, scale by `2^frac_bits`, truncate
toward zero — the unsigned 8.8 scalars (`max_age`/`start_age`/the burst fields/
`part_max_age`), the unsigned 3.5 angles and scales, and the signed 8.7
angular-velocity / acceleration vectors (bias `+2^int_bits` then scale); glow is
`trunc(value · 255)`. Exported from `sl-proto` alongside the decoders; no
runtime wiring (server-side binary sub-codecs, reused by #57's `ObjectUpdate`
body assembly and #60's `SimSession`). Covered by four new `particles.rs` tests
(the
texture-anim `decode`→`encode` byte identity plus an `encode`→`decode` value
round trip; a legacy `encode`→`decode` full-struct round trip over
exactly-representable values that asserts the 86-byte length; a modern round
trip exercising glow+blend and a glow-only form with the matching size checks;
and `decode`→`encode` byte-for-byte idempotency over the hand-built modern
blob). *Test: unit round-trip (no grid).*
