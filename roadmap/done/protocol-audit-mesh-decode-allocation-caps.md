---
id: protocol-audit-mesh-decode-allocation-caps
title: Mesh decode: unbounded inflate, unbounded header offset, unchecked indices
topic: protocol
status: done
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

## Fixed (2026-08-27)

Two ceilings in `sl-mesh/src/decode.rs`, both generous enough that no
legitimate asset is refused:

- `MAX_INFLATED_BLOCK_BYTES` (64 MiB) — `inflate` reads through
  `Read::take(limit + 1)` and errors with the new
  `MeshDecodeError::InflatedTooLarge` if it overshoots. Reading *one byte past*
  the limit is what detects it: the cap is itself a legal length, so stopping
  exactly at it would be indistinguishable from a block that simply is that
  big.
- `MAX_MESH_ASSET_BYTES` (64 MiB) — `block_ref` rejects any block whose
  `offset + size` lands beyond the largest asset we will hold, so a header
  claiming `offset: 2000000000` never reaches the on-disk assembler that would
  zero-fill up to it. `i32::MAX` alone was not a bound; it is two gigabytes of
  zero-fill per mesh.

Also here, both from the same audit entry:

- `decode_indices` now takes the submesh's vertex count and drops any triangle
  naming a vertex it does not have. Whole triangles rather than clamped
  indices: a clamped index silently welds a face to an unrelated vertex, which
  is worse than a missing one.
- A *present but negative* `version` is refused instead of folding to `0` via
  `unwrap_or`, which walked it straight past the `MAX_MESH_VERSION` guard on
  the next line. Only an absent field defaults now.

Five tests, including a 256 MiB compression bomb and a `2_000_000_000` offset.

**Split out, not fixed here:** validating a rig's joint indices against the
skin's joint count. `decode_weights` has no access to `MeshSkin::joint_names`
— the two live in different blocks of the asset and never meet inside
`sl-mesh` — so the check belongs at the consumer that joins them. See
[[protocol-audit-mesh-joint-index-bounds]].
