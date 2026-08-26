---
id: protocol-audit-j2c-dimension-caps
title: No MAX_IMAGE_SIZE / MAX_IMAGE_AREA cap on a JPEG-2000 header
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/j2c.rs:278` reads `width` / `height` as raw `Xsiz-XOsiz` /
`Ysiz-YOsiz` u32s and applies no cap. The reference clamps to `MAX_IMAGE_SIZE`
/ `MAX_IMAGE_AREA` / `MAX_IMAGE_DATA_SIZE` (`indra/llimage/llimage.h:56-60`,
4096 and 128 MB).

Two consequences downstream:

- `sl-texture/src/store.rs:401` — `ensure_codestream` loops calling `fetch_more`
  until `covered >= need`, where `need` is `header.full_data_size_bound()` =
  `width * height * components` saturating to `usize::MAX` (`j2c.rs:88`). A
  header field therefore drives unbounded in-RAM accumulation.
- `sl-texture/src/decode.rs:147` — `decode_j2c` hands the uncapped codestream to
  OpenJPEG with no pre-check; `to_rgba8` (`:287`) then allocates `w * h * 4`.
  `reduce()` only divides an already-unbounded native size.

Porting the reference's named constants closes both. Related but separate:
`decode.rs:374`, where `downsample` stamps `discard_level: target` even when the
halving loop broke early at `width <= 1 || height <= 1`, so a small image is
returned claiming a level it never reached — any store or budget logic keyed on
`discard_level` is then working from a wrong label.
