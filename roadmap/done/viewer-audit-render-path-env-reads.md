---
id: viewer-audit-render-path-env-reads
title: About a dozen getenv calls and allocations per frame in the sky and post chain
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`day_position` (`sl-viewer-world-scene/src/sky.rs:1988`) calls
`std::env::var("SL_VIEWER_SKY_DAY_POSITION")` — allocating a `String` and taking
the process env lock — on **every invocation**, and it is called seven times per
frame (`sky.rs:707`, `:945`, `:1191`, `:1421`, `terrain.rs:94`, `water.rs:321`,
`underwater_fog.rs:153`).

Add `sky_linearize` / `resolved_sky_hdr_scale` per params build,
`exposure.rs:402` (twice up front plus once per camera at `:440`),
`tonemap.rs:205`/`:210`/`:215` and `glow.rs:146`/`:151` — the last two **inside
per-camera loops** — and it is roughly a dozen env reads per frame.

The workspace already has both fixes: `OnceLock` (`physics.rs:2312`) and
`Local<Option<bool>>` (`particles.rs:698`). Resolve each of these once.

## Resolved

Every render-path knob is now resolved **once per process** behind an
`OnceLock`, following `physics.rs`'s precedent. No `std::env::var*` call
remains on a per-frame path in `sl-viewer-world-scene`: the only reads left
are inside a `OnceLock` initialiser, a `Default` / `FromWorld`, a
`Plugin::build`, or `beacons.rs`'s once-per-state-flip log gate.

- `sky.rs` — `day_position`'s override moved into `pinned_day_position()`, so
  the seven per-frame callers (sky / cloud / star / disc drives, `terrain.rs`,
  `water.rs`, `underwater_fog.rs`) cost a load instead of a `String`
  allocation and the process env lock. Likewise `sky_linearize`, a new
  `sky_hdr_scale_override()` behind `resolved_sky_hdr_scale`, and
  `shadow_cascade_count` / `sun_shadows_enabled`. The
  `SL_VIEWER_LOG_SKY_HDR` and `SL_VIEWER_LOG_CLOUDS` gates (three per-frame
  test sites each) became `log_sky_hdr()` / `log_clouds()`.
- `exposure.rs`, `tonemap.rs`, `glow.rs` — one `…Overrides` struct per
  module, read once, replacing the per-camera-loop `var_os` tests.
  `TonemapOverrides` carries the parsed `SL_VIEWER_TONEMAP_MIX` value rather
  than a flag, since the old code re-parsed it inside the loop whenever it was
  set; a set-but-unparsable value still wins over the stored setting, at the
  reference default.
- `underwater_fog.rs` — `fog_disabled()`; `probes.rs` — `probe_ambient_scale`,
  `probe_gain` (read from two per-frame systems), `probe_test_sphere_enabled`.
- `environment.rs`'s synthesised-day-cycle gate now tests the *resolved*
  `pinned_day_position()` rather than the variable's mere presence, so the
  synthetic cycle is installed exactly when the pin will drive it.

No behaviour change beyond that last gate: the knobs are set before launch
and never change within a run. Verified by
`cargo clippy --workspace --all-targets` (clean) and the crate's 112 unit
tests.
