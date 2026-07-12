---
id: idiomatic-p7-11
title: directory.rs/avatar_profile.rs global-metre fields → NEW public client
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`directory.rs`/`avatar_profile.rs` global-metre fields →
    NEW public client-local `GlobalCoordinates` newtype
    (`sl-wire/src/geometry.rs` — see the second-pass note): grid-**global**
    metres, `f64`-backed
    (matching the wire's `LLVector3d` double-precision globals). MAXIMAL scope
    (user-directed 2026-06-24): `PlacesResult.global_position` (the F32 wire
    field widens to `f64` at the boundary via `f64::from`, narrows back via a
    sanctioned `global_to_f32` on the server encode),
    `EventInfo.global_position` (f64), and the four `avatar_profile.rs`
    `pos_global` fields
    (`PickInfo`/`ClassifiedInfo`/`PickUpdate`/`ClassifiedUpdate`, all f64).
    `new`/`x()`/`y()`/`z()` + conversions to/from a `(GridCoordinates,
    RegionCoordinates)` pair (`from_grid_and_region`/`From<(…)>`/`split() ->
    Option<(…)>`, the `region_index * 256 + region_local` mapping) plus a
    region-**corner** shortcut `from_grid_corner(grid)` /
    `From<GridCoordinates>` (`<256 * grid_x, 256 * grid_y, 0>`, no all-zero
    `RegionCoordinates` — user-requested 2026-06-24). Re-exported through
    `sl-proto`/tokio/bevy; REPL `global_or_zero` returns `GlobalCoordinates`.
    +7 geometry unit tests. NO `sl-types` change.

    **`sl-types` migration note (PPS HUD config): DONE in `sl-types 0.6.0`.**
    `GlobalCoordinates` moved into `sl-types` and
    `GridRectangleLike::pps_hud_config` now builds its global-metre LSL vector
    through `GlobalCoordinates::from_grid_corner` (see the second
    batched-migration subsection above).
