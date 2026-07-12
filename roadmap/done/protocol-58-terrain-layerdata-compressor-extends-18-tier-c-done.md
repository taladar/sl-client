---
id: protocol-58
title: Terrain LayerData compressor (extends #18, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**58. Terrain `LayerData` compressor (extends #18, Tier C). ✅ Done.**
`terrain.rs` gains `encode_layer` (exported from `sl-proto`), the inverse of
`decode_layer`: prescan each patch for its range/DC-offset, scale heights onto
the `2^PREQUANT` (=10, as the viewer/OpenSim use) quantizer grid, run a forward
DCT, quantize+zigzag the coefficients into transmission order, choose the
minimal lossless `word_bits`, and entropy-code them (`0`=zero,
`10`=end-of-block, `11`+sign+magnitude), framed by the group header
(`stride`/`size`/layer
code) and a trailing `END_OF_PATCHES` written through a new MSB-first
`BitWriter` (the exact inverse of the decoder's `BitReader`). The **forward DCT
is the exact algebraic inverse of the decoder's `inverse_dct`** — `B =
(2/size)·E·spatial·Eᵀ` with the same `icosines` table and `w(0)=1/√2`,
`w(u>0)=1` weights, so the single `2/size` and both weights mirror the decoder
rather than re-deriving OpenSim's hardcoded 16×16 Ooura routine; this handles
the 32×32 extended (variable-region) patches too, which the OpenSim reference
does not. The patch-coordinate width (10 vs 32 bits) follows
`layer.is_extended()`, the cell-grid size comes from each patch's `size`. NO
runtime wiring (terrain is sim-pushed; this is a server-side encoder). Five
round-trip tests in `terrain.rs` (flat patch near-lossless; smooth ramp+bump
within the quantization tolerance; multi-patch coordinate preservation; a 32×32
`LandExtended` patch with a large 32-bit patch X; and decode→encode→decode
stability) plus the existing decoder/lifecycle tests unchanged. NB the public
`encode_layer` doc uses a plain code span for the `pub(crate)` `decode_layer`,
not an intra-doc link (the `cargo doc` `private_intra_doc_links` `-D` check,
same as #55–#57). *Test: unit round-trip (no grid); the local OpenSim's live
`LayerData` already exercises the matching decoder (#18).*
