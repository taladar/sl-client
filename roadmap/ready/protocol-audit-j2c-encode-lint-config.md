---
id: protocol-audit-j2c-encode-lint-config
title: The one crate with unsafe FFI has the workspace's weakest lint configuration
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/protocol.md](../context/protocol.md).

`sl-j2c-encode/Cargo.toml`'s `[lints.clippy]` sets only
`undocumented_unsafe_blocks = "deny"`, opting out of the ~200 workspace
restriction lints in order to disable `unsafe_code = "forbid"`.

The cost is visible at `sl-j2c-encode/src/lib.rs:120-121` and `:171-172`:
`(width as usize).checked_mul(height as usize)` — bare `as` casts that would be
rejected anywhere else in the workspace.

`sl-cef/Cargo.toml` shows the right shape: it makes the same opt-out but
explicitly **re-denies** `as_conversions`, `indexing_slicing`, `unwrap_used`,
`expect_used` and `allow_attributes*`. Copy that.

For the record, the `unsafe` itself is sound and well documented: every block
carries an accurate SAFETY comment, the pixel-copy loop is sound because
`encode_rgba8` (`:124`) enforces `pixels.len() == width * height * 4` exactly
(with `!=`, not `>=`), and the RAII `Codec` / `Image` / `Stream` drop order is
correct on every early-return path (stream before the boxed `MemStream` it
points at).

The tests this item also asked for exist now (audit 2026-10-15):
`sl-texture/src/encode.rs` round-trips `encode_j2c` (the wrapper over
`sl-j2c-encode`) through `decode_j2c` for an opaque image, one with alpha and
a five-component bake, and `sl-texture/src/decode.rs` has tests of its own.
What remains is the lint configuration above.
