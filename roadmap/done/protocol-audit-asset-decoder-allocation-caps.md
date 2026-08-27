---
id: protocol-audit-asset-decoder-allocation-caps
title: Notecard, animation and legacy-material decoders reserve from a wire-supplied count
topic: protocol
status: done
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

## Fixed (2026-08-27)

The same shape everywhere: a count off the wire may no longer size an
allocation on its own. What the *read loop* finds stays the authority on how
much is actually there, so nothing that used to decode is refused now — a
`Vec` that guessed low simply grows.

- **`sl-notecard`** — a free `reserve_hint(count, remaining)` bounds the
  embedded-item reservation by what the unread bytes could hold, using a
  `MIN_EMBEDDED_ITEM_BYTES` floor of 8 (the shortest well-formed entry is about
  34 bytes). Seventy bytes of notecard panicked with a capacity overflow
  before; the test that pins this fails without the bound.
- **`sl-anim`** — `Cursor::reserve_hint` with a `MIN_KEY_BYTES` floor of 8 (the
  modern encoding's `u16` time plus three `u16` components; the legacy `0.1`
  encoding costs twice that). The reference does not preallocate on key counts
  at all.
- **`sl-avatar/basemesh`** — the same `Cursor::reserve_hint`, applied to all
  seven reservation sites rather than only the two the audit named: the vec3 /
  vec2 / f32 arrays, faces, names, morph deltas and the shared-vertex remaps.
  Each passes its own per-element wire size.
- **`sl-wire/material/legacy`** — both `RenderMaterials` paths now inflate
  through `decompress_to_vec_zlib_with_limit` at `MAX_INFLATED_MATERIALS_BYTES`
  (64 MiB), matching the ceiling
  [[protocol-audit-mesh-decode-allocation-caps]] set for mesh blocks.

Five tests. Four of them fail (three by aborting the process) without the
change; the fifth checks `reserve_hint` directly, because the animation case's
42 GB reservation is one Linux overcommit hands out without complaint, so the
end-to-end test alone would not have caught it.

Noted while here, not fixed: `legacy.rs`'s duplicate binary-LLSD reader
recurses with no depth bound, so the inflate ceiling still allows deep-enough
nesting to overflow the stack. `sl-llsd` gained `MAX_NESTING_DEPTH` in
[[protocol-audit-llsd-recursion-depth-cap]]; routing this codec through it is
[[protocol-audit-legacy-material-date-codec]]'s scope.
