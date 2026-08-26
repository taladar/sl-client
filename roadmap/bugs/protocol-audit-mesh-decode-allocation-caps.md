---
id: protocol-audit-mesh-decode-allocation-caps
title: Mesh decode: unbounded inflate, unbounded header offset, unchecked indices
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/protocol.md](../context/protocol.md).

`sl-mesh` decodes attacker-supplied blobs from the asset CDN and has three
missing bounds:

- `sl-mesh/src/decode.rs:619` — `inflate` is `ZlibDecoder::new(compressed)` plus
  `read_to_end` into an **uncapped** `Vec`, on all three public entry points
  (`decode_lod:349`, `decode_skin:510`, `decode_physics_convex:560`). Roughly
  1 MB of zeros inflates to ~1 GB. `miniz_oxide` already offers
  `decompress_to_vec_zlib_with_limit`.
- `sl-mesh/src/disk.rs:101` — `with_block` does
  `Vec::with_capacity(self.data.len().max(end))` then `grown.resize(end, 0)`,
  where `end` derives from `BlockRef::range()`. `block_ref`
  (`decode.rs:310`) accepts any `offset` up to `i32::MAX` with **no ceiling and
  no check against the asset's actual length**, so a header claiming
  `offset: 2000000000` zeroes 2 GB per mesh. Reached unconditionally from
  `sl-mesh/src/store.rs:477`.
- `sl-mesh/src/decode.rs:452` — `decode_indices` only truncates to a multiple of
  three; nothing checks an index against the vertex count, and the consumer
  (`sl-client-bevy/src/meshes.rs:58`) hands them straight to
  `mesh.insert_indices`. Rig joint bytes (`decode.rs:467`) have the same gap,
  and `pack_influences` does not close it either.

Also here: `decode.rs:265` turns a hostile `version: -1` into a valid version 0
via `u32::try_from(...).ok().unwrap_or(0)`, bypassing the `MAX_MESH_VERSION`
guard on the next line.
