---
id: protocol-audit-conversions-test-coverage
title: conversions.rs has 233 pure functions and 32 tests
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-lure-region-handle]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/conversions.rs` is 227-233 small pure functions —
`&[u8]` / `Llsd` to struct converters with no I/O and no session state — and the
largest single test gap in the crate: roughly **180 are never named in the test
module**.

Highest-value untested ones:

- `sky_settings_from_asset` (`:808`), `water_settings_from_asset` (`:823`),
  `environment_asset_from_bytes` (`:840`), `strip_llsd_header_line` (`:874`),
  `day_cycle_from_llsd` (`:886`), `track_from_llsd` (`:929`);
- `parse_mute_list` / `parse_mute_line` (`:195`, `:207`);
- `pack_uuids` / `unpack_uuids` (`:141`, `:152`);
- `compute_im_session_id` (`:1325`);
- `region_handshake_message` (`:663`);
- `parse_lure_region_handle` (`:165`) — which is where
  [[protocol-audit-lure-region-handle]] lives.

These need no grid and no async runtime. Note `sl-conformance` does not depend
on `sl-proto` directly, so **none** of its 95 live cases can pin any of them —
everything here is offline-testable today with nothing new.

Three more offline gaps in the same crate worth folding in:

- `src/terrain.rs` — the variable-region 32-edge decode path is untested.
  `decode_layer:171` branches on `large = layer.is_extended()` selecting 32-bit
  vs 10-bit patch ids, and `build_decopy_matrix:396` / `build_icosine_table:378`
  are parameterised on patch size 16 vs 32. Only the **encode** side is covered;
  the decode side is observed solely via the live `terrain_composition` case.
- `src/extra_params.rs:289` / `:247` — `decode_extra_param_blocks` and
  `extra_param_message_blocks` are not mentioned in the file's test module at
  all; the 7 tests cover only the packed-blob path.
- `src/j2c.rs:278` — 10 tests, all built from one `synth_header` helper, with a
  single negative assertion. `find_marker` (`:263`), `read_u16_be` (`:246`) and
  `read_u32_be` (`:253`) get no truncated, missing-marker or garbage input.
