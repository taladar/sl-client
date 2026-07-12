---
id: idiomatic-p7-12
title: Second pass — completeness sweep (2026-06-24)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Second pass — completeness sweep (2026-06-24, user-directed "everything
    incl. sl-wire fields").** The first pass missed several coordinate-shaped
    fields that did not use the `position`/`pos_global`/`global_position`
    naming the audit grepped for. Decision (user `AskUserQuestion`): convert
    **all** of them, and **move `Direction`/`GlobalCoordinates` DOWN into
    `sl-wire`** (alongside `RegionHandle` etc.) so the wire codec layer can
    use them too — `sl-proto` re-exports them, so the flat `sl_proto::…`
    surface is unchanged. `RegionCoordinates` was already reachable in
    `sl-wire` (it lives in `sl-types`). Additional conversions:
    - **`sl-proto`:** `ParcelUpdate.user_location`→`RegionCoordinates` /
    `user_look_at`→`Direction` (were `lsl::Vector`); `Postcard.pos_global`
    (`[f64;3]`)→`GlobalCoordinates`; `ParcelDetails` `global_x/y/z`
    (f32)→one `global_position: GlobalCoordinates`; `LandStatItem.location`
    (`[f32;3]`)→`RegionCoordinates`; `ViewerEffectData`
    `LookAt`/`PointAt.target_position` + `Spiral.position`
    (`[f64;3]`)→`GlobalCoordinates`; and
    `Command::RequestRemoteParcelId.location` (`Vector`)→
    `RegionCoordinates`.
    - **`MapItem`** (`global_x`/`global_y`: `u32`)→one `position:
    GlobalCoordinates` (z=0). The old `& !0xFF` / `& 0xFF` C-ism is gone:
    region/offset split now goes through `GlobalCoordinates::split()`, and
    `region_handle()`/`region_position()` return `Option` (the typed split
    is fallible). `MapItem` lost `Eq` (now holds `f64`). Codec narrows
    `f64`→`u32` via a sanctioned `map_global_to_u32`.
    - **`sl-wire`:** `HomeLocation.position`→`RegionCoordinates` /
    `look_at`→`Direction`; `LoginSuccess.look_at` (and the mirrored
    `LoginAccount.look_at`)→`Option<Direction>`;
    `StartLocation::Region.position`→`RegionCoordinates`;
    `RemoteParcelRequest.location` (`[f64;3]`)→`RegionCoordinates` (LLSD
    `f64` reals narrow at the boundary via the now-`pub(crate)`
    `geometry::narrow`); `ScriptedObjectInfo.location`
    (`[f32;3]`)→`RegionCoordinates`. `RemoteParcelRequest` lost its derived
    `Default` (manual impl — `RegionCoordinates` is not `Default`).
    `HomeLocation.region_handle` (`(u32,u32)` corner metres)→`RegionHandle`
    (the existing handle type; codec uses `from_global`/`global_coordinates`).
    - **Third pass — environment value types (user-directed: "not all vectors
    of 3 numbers are the same").** The `environment.rs` `[f32;3]` fields were
    *not* coordinates but still deserved distinct types so they can't be
    transposed. NEW client-local value newtypes in `environment.rs` (migration
    candidates): `Color`
    (RGB — `ambient`/`blue_horizon`/`blue_density`/
    `cloud_color`/`water_fog_color`); `Scale` (a dimensionless 3-axis
    **scale factor**, NOT metres — `normal_scale`, the water "Reflection
    Wavelet Scale", verified vs the viewer `WATER_NORM_SCALE` uniform); `Glow`
    (`(size, reserved, focus)` — the unused middle is preserved verbatim for
    round-trip); `CloudPosDensity` (`(position_x, position_y, density)` — a
    *mixed* vector with named accessors, since Z is density not altitude);
    `ColorAlpha`
    (RGBA — `sunlight_color`, the alpha-carrying `[f32;4]` sibling of `Color`,
    kept distinct so it can't be transposed with a 3-channel colour or a
    rotation quaternion). LLSD codec gained
    `{color,color_alpha,scale,glow,cloud_pos_density}_{from,to}_llsd` helpers;
    `color3_from_llsd` removed. +5 unit tests. Re-exported through `sl-proto`
    (not the runtimes — the sibling `SkySettings`/`WaterSettings` aren't
    re-exported there either).
    - **`MapLayer` `left/right/top/bottom`: DONE in `sl-types 0.6.0`.** These
    grid-index rectangle bounds are now one `rect: GridRectangle`, after
    `GridRectangle`/`GridCoordinates` were widened `u16` → `u32` (the SL
    whole-grid layer reports bounds exceeding `u16::MAX`). See the second
    batched-migration subsection above. `EnvironmentSettings.track_altitudes`
    (three scalar altitude breakpoints, not a vector) stays raw.
    Re-exported `Direction`/`GlobalCoordinates` through `sl-proto`/tokio/bevy;
    `Color`/`ColorAlpha`/`Scale`/`Glow`/`CloudPosDensity` through `sl-proto`;
    REPL + survey updated; wire bytes byte-identical throughout. All builds +
    clippy(0) + 742 tests + doc(0) + rumdl green. NO `sl-types` change.
