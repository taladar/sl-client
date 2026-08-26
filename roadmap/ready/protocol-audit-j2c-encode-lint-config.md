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

The cost is visible at `sl-j2c-encode/src/lib.rs:91-92`:
`(width as usize).checked_mul(height as usize)` — bare `as` casts that would be
rejected anywhere else in the workspace.

`sl-cef/Cargo.toml` shows the right shape: it makes the same opt-out but
explicitly **re-denies** `as_conversions`, `indexing_slicing`, `unwrap_used`,
`expect_used` and `allow_attributes*`. Copy that.

For the record, the `unsafe` itself is sound and well documented: every block
carries an accurate SAFETY comment, the pixel-copy loop at `:275-278` is sound
because `encode_rgba8:95` enforces `pixels.len() == width * height * 4` exactly
(with `!=`, not `>=`), and the RAII `Codec` / `Image` / `Stream` drop order is
correct on every early-return path (stream before the boxed `MemStream` it
points at).

Missing tests, while there: the 3 existing ones are encode-only and none checks
the codestream is **decodable**. `encode_rgba8` -> `sl_texture::decode_j2c` ->
pixels-within-tolerance is a genuine round trip needing no live grid (the two
crates are in the same workspace), and the non-opaque 4-component path
(`:228-233`) is currently unverified. That same round trip is also the missing
`sl-texture` test — `decode_j2c` has **no test at all** today.
