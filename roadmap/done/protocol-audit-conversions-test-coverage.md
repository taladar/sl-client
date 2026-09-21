---
id: protocol-audit-conversions-test-coverage
title: conversions.rs has 233 pure functions and 32 tests
topic: protocol
status: done
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

**Done** (2026-09-20). 25 offline tests, over every function this file named
plus the three sibling codecs. They are grouped where the code is, not where
the old test module happened to be: a new `settings_asset_tests` beside the EEP
decoders, the rest beside their own subject.

The audit's count was measured against the file's *own* test module, which
undercounts — `environment_asset_from_bytes` and `region_identity` were already
pinned from `sl-proto/tests/lifecycle.rs`, and `parse_mute_line` and
`parse_lure_region_handle` from the two inline modules. What was genuinely
untested is now covered:

- **The settings-asset triple.** The `type` tag is what tells a sky, a water
  frame and a day cycle apart, so each decoder is shown refusing the other two
  kinds and an untagged map, rather than returning a frame assembled from absent
  members. The caller's name wins over the payload's own for all three (a day
  cycle's `name` is overwritten for exactly that reason). Both frame decoders
  are pinned on their own: the **three-place legacy-haze lookup** (`legacy_haze`
  sub-map, then the frame, then the built-in default — the middle and last of
  which are what a frame saved by an older reference viewer needs, and whose
  absence once decoded as a black, hazeless sky), the density-profile fallback
  that keeps a re-encoded sky from being one the reference throws away, and
  water's flat zero-default with its nil-texture sentinel.
- **Day cycles are positional.** Track 0 is water and the rest are sky tracks,
  ground up; frames share one name namespace and are sorted by each frame's own
  `type`, with an untagged frame read as a sky. An absent or malformed cycle
  decodes to an empty one, a non-array track to no keyframes.
- **The two buckets and the IM session id.** `pack_uuids`/`unpack_uuids` invert,
  and a trailing partial chunk is dropped rather than padded into an invitation
  for the nil agent; the give-inventory bucket rejects an asset type that does
  not fit its byte instead of mistagging the offer as a texture; the 1:1 session
  id is symmetric (both sides compute it without being told) and an IM to
  oneself keeps the agent id rather than going nil.
- **`RegionHandshake` round-trips through `RegionIdentity`** — name, 64-bit
  extended flags, maturity, product classification and the terrain compositing
  parameters — so the simulator side answers with what it was told.
- **A mute-list file** parses in order, skips blanks, accepts the flagless line
  shape, and fails *whole* on one bad line (a partially-applied list silently
  un-mutes somebody).
- **Terrain**: the extended decode path read off a hand-built 32×32 payload
  (not our own encoder's output), the un-zigzag matrix proved a permutation at
  both patch sizes, the icosine table checked against `cos((2n+1)u·π/2/size)` at
  both, the bit reader's overrun latch, and *every* prefix of a two-patch
  message shown to decode to a bounded result with a full `size*size` grid.
- **`ObjectExtraParams` blocks**: the message form carries all seven subtypes
  with payloads byte-identical to the packed container's, round-trips back, and
  a not-in-use or unknown-subtype block clears rather than carries.
- **J2C**: the two big-endian field readers refuse to read past the end
  (including at `usize::MAX`), `find_marker` takes the first match and stops at
  the scan window's edge, every truncation of a `SIZ` segment and a canvas
  origin outside its canvas are non-headers, and a codestream with no `COD`
  clamps every LOD request to full resolution.

What is left uncovered is the long tail of one-line block converters
(`group_role`, `map_item`, `avatar_group`, …), which restate their wire block
field by field; they are worth a sweep only if one of them is found wrong.

One residual observation, judged not worth acting on: `find_marker` scans for a
raw `0xFF52` byte pair, so a `SIZ` field value containing those two bytes before
the real `COD` segment would be read as the marker and `decomposition_levels`
taken from the wrong byte. Every canvas field that could hold it is already out
of range for `Header::within_limits`, the tile fields are not, and the cost of
being wrong is one LOD step — a real parse of the segment lengths would be the
fix if a texture in the wild ever shows it.
