---
id: protocol-audit-asset-decoder-allocation-caps
title: Notecard, animation and legacy-material decoders reserve from a wire-supplied count
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/protocol.md](../context/protocol.md).

Three decoders size an allocation directly from an attacker-controlled field:

- `sl-notecard/src/decode.rs:393` — `Vec::with_capacity(count)` where `count`
  comes from `parse_usize` over asset text with no upper bound and no
  cross-check against `cursor.remaining()`. About 70 bytes reaches a
  capacity-overflow panic or a multi-GB request. **Parcel covenants decode
  through this path** (`sl-viewer-places/src/about_land.rs:1617`). The reference
  rejects only `count < 0` and never preallocates (`llnotecard.cpp:82`).
- `sl-anim/src/decode.rs:438` and `:444` — `Vec::with_capacity` on
  `num_rot_keys` / `num_pos_keys`, which `read_key_count` (`:459`) only checks
  for negativity. An `i32::MAX` key count reserves ~42 GB from a tiny file. This
  is a **deviation from the reference**, which caps joints but does not reserve
  on key counts at all (`llkeyframemotion.cpp:1548`).
- `sl-wire/src/material/legacy.rs:97` and `:187` — both `RenderMaterials` paths
  inflate the `{"Zipped": ...}` blob with `decompress_to_vec_zlib` and no output
  cap.

Lower severity but the same shape, worth the same one-line fix:
`sl-avatar/src/basemesh.rs:717` and `:738` reserve a `u32` read from the local
`.llm` (`MorphDelta` is ~64 bytes, so `u32::MAX` is ~256 GB) — those files come
from the local Firestorm install rather than the grid.
