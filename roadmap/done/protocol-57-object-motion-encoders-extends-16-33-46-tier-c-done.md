---
id: protocol-57
title: Object-motion encoders (extends #16/#33/#46, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**57. Object-motion encoders (extends #16/#33/#46, Tier C). ✅ Done.** The
motion decoders (`full_object_motion`, `terse_update`, `compressed_object` +
`compressed_object_trailing`, with their `read_quantized_vector` /
`read_compressed_shape` / `read_nul_string` helpers and the `COMPRESSED_*`
flags) were extracted out of `session.rs` into a new
`sl-proto/src/object_update.rs`, co-located with the new inverse encoders:
`encode_object_motion` (the full-precision `ObjectUpdate` `ObjectData` blob),
`encode_terse_object_data` (the `ImprovedTerseObjectUpdate` `Data` blob),
`encode_terse_texture_entry` (the wrapped terse `TextureEntry` field), and
`encode_compressed_object` (the `ObjectUpdateCompressed` `Data` blob). The
public `TerseUpdate` struct and all four encoders are exported from `sl-proto`;
the decoders stay `pub(crate)` and `object_from_full_update` /
`shape_from_full_block` (full-update glue using session's `trimmed_string`) stay
in `session.rs`, calling into the new module. The 16-bit terse fields
re-quantize with the round-tripping `F32_to_U16_ROUND` (LL's plain `F32_to_U16`
floors and can re-encode one quantum short); the compressed encoder computes the
`CompressedFlags` from which fields the `Object` carries (non-empty `data`
always as the scratchpad form; an 86-byte particle blob as legacy, else "new")
and emits the raw `texture_entry` / `texture_anim` / `particle_system` /
`extra_params` byte fields a server assembles via #54/#55/#56 (the `ExtraParams`
container is rebuilt from the decoded `extra` via #55 when the raw field is
empty, so it is always a valid framed block). NO runtime wiring beyond rerouting
the existing decode call sites; `ZERO_VECTOR` / `IDENTITY_ROTATION` made
`pub(crate)` for sharing. Six round-trip tests in `object_update.rs`
(full-motion 60/76-byte byte-identity, terse byte-identity over grid-point
quantized values,
the terse texture-entry wrapper, and rich + minimal compressed-object
decode→encode→decode round trips with byte identity). NB the public encoder docs
must use a plain code span for the `pub(crate)`/private decoders (`f32_to_u16`,
`compressed_object`, …), not an intra-doc link (the `cargo doc`
`private_intra_doc_links` `-D` check, same as #55/#56). *Test: unit round-trip
(no grid).*
