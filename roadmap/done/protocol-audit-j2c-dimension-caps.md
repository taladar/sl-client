---
id: protocol-audit-j2c-dimension-caps
title: No MAX_IMAGE_SIZE / MAX_IMAGE_AREA cap on a JPEG-2000 header
topic: protocol
status: done
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

## Fixed (2026-08-27)

The reference's four constants are now `sl-proto/src/j2c.rs`'s
`MAX_IMAGE_SIZE` / `MAX_IMAGE_AREA` / `MAX_IMAGE_COMPONENTS` /
`MAX_IMAGE_DATA_SIZE`, and a test pins them to the identities they are derived
from so the literals cannot drift apart.

They are applied in three places, deliberately overlapping:

- `parse_header` is the gate. It returns `None` for a `SIZ` segment declaring a
  degenerate or over-cap image, so an unusable geometry never becomes a
  `Header` that a caller can size work from. Both HTTP fetchers and the texture
  store already treat `None` as "not a recognisable codestream" and stop at the
  600-byte probe, so the unbounded `fetch_more` growth loop is closed without
  touching them.
- `full_data_size_bound` and `discard_data_size` cap the pixel-byte product at
  `MAX_IMAGE_DATA_SIZE` regardless of how the `Header` was built. The gate is
  the policy; this is the arithmetic being unable to produce an unbounded number
  in the first place, since `Header`'s fields are public.
- `decode_j2c` refuses an out-of-range header with a new
  `DecodeError::OutOfRange` **before** OpenJPEG sees the codestream — the
  decoder allocates from `SIZ` too, and `to_rgba8` then allocates `w * h * 4`
  on top. The pre-check reads the header through the new
  `parse_header_unvalidated`, which exists precisely so the decoder can tell
  "not a J2C at all" (leave it to OpenJPEG's codec error) from "a J2C claiming
  an image too large to decode" (report which numbers, and why).

Also here, the `downsample` mislabel: it now stamps the level it actually
reached, not the one it was asked for. `store.rs`'s `downgrade` gained the
matching guard — a 1-pixel-wide or -tall image has no halving left, so the CPU
task is skipped rather than repeated on every coarser request. The upgrade path
already labelled honestly via `achieved_discard`; the downgrade path now
matches it.

Seven tests across the two crates, including a header claiming `u32::MAX` in
both axes and one claiming 65536² with 8 components.
