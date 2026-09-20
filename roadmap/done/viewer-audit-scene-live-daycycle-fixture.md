---
id: viewer-audit-scene-live-daycycle-fixture
title: A test fixture that advances the day cycle between samples
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-scene-change-guards-day-cycle, viewer-audit-probe-ambient-multiply, viewer-audit-tonemap-legacy-sky]
---

Context: [context/viewer.md](../context/viewer.md).

Three separate scene defects — [[viewer-audit-scene-change-guards-day-cycle]],
[[viewer-audit-probe-ambient-multiply]] and [[viewer-audit-tonemap-legacy-sky]]
— are all **correct under the screenshot harness and wrong on a live grid**,
because the harness pins `SL_VIEWER_SKY_DAY_POSITION` and a real grid does not.

That is a coherent blind spot rather than three unrelated slips, and it argues
for a fixture that advances `day_position` between samples so a guard which
never holds under a moving sky is a test failure rather than a profiler finding.

Scope: a scene test app that steps the day cycle across frames and asserts
(a) `TerrainMaterial` is **not** marked modified when the lighting has not
meaningfully changed, (b) `GlobalAmbientLight` converges rather than decaying
under a non-zero `probe_ambient_scale`, and (c) a legacy sky produces a zero
tonemap mix.

(a) and (b) now have pure-level tests from the two fixes that landed
([[viewer-audit-scene-change-guards-day-cycle]] quantises the day position and
samples `terrain_lighting` across a moving cycle;
[[viewer-audit-probe-ambient-multiply]] asserts the ambient is bit-identical
across frames under a non-zero share). What is still missing here is the
**app-level** version — a real `App` whose `GlobalAmbientLight` and
`TerrainMaterial` change ticks are the thing asserted, so a future system that
reintroduces a per-frame write fails a test rather than a profile.

Zero-test scene files this would give a home to: `water.rs` (638 lines — with
the pure `reconcile_region_planes:388` region-set diff and `water_params`),
`glow.rs` (619), `underwater_fog.rs` (427), `environment.rs` (328).

## Done (2026-09-20)

`sl-viewer-world-scene/src/day_cycle_fixture.rs` — a `#[cfg(test)]` module, so
it compiles for `cargo test` and for nothing else. `DayCycle` stands up a
headless app running the crate's real `SkyPlugin` (dome, both suns, discs,
clouds, stars, ambient) plus `refresh_tonemap_settings` over a ground-level
camera, and steps it with **region time advancing a fixed slice per frame**.
Seven tests, 0.06 s for the lot.

### What it observes, and why that is the whole point

The three defects were not arithmetic mistakes; each was a **write** that
should not have happened. So the fixture records, per frame, exactly the four
signals the renderer itself reacts to — `GlobalAmbientLight`'s change tick, the
scene sun's `Transform` / `DirectionalLight` change ticks, and
`AssetEvent::Modified` for the sky material, the shared `SKY_LIGHTING_IMAGE`
and every `TerrainMaterial`. A pure function has no write to observe, which is
why the three fixes' pure tests could not state any of this.

### It owns the clock rather than reading one

`day_position` reads `SystemTime::now()`, so an app driven by it would settle
or not depending on how close the machine happened to start to a sampling-cell
boundary — a flake on the one assertion that matters most. The fixture instead
computes each frame's position from its own simulated region time with the
crate's real `quantised_day_position` and pins *that*. Downstream a pinned
position and a clock-derived one are the same value through the same code, but
this pin **advances between frames**, which is the one thing the screenshot
harness's does not do and the entire reason the three defects survived it.

The environment arrives the way a region's does, as an
`SlSessionEvent::Environment` folded in by the real `ingest_environment`, over
the four ported presets keyframed across a four-hour day — the shipped
single-frame default returns the same noon frame at every position and would
have proved nothing. That cycle helper moved here from `sky.rs`'s test module,
which now imports it: the pure assertions and the app-level ones have to be
sampling the same cycle or neither says anything about the other.

`refresh_tonemap_settings` is left **unordered** against the sky fold, as the
viewer's own schedule leaves it, so the fixture does not quietly test a
schedule the viewer does not run.

### The assertions, and what each one would have caught

- **(a) the scene writes nothing between day-cycle steps.** Stated as a budget
  over region time — at most one writing frame per sampling cell crossed — not
  as "a frame whose pinned position repeated wrote nothing", because the pin is
  the quantiser's own output and the second phrasing would go vacuous the
  moment the quantiser stopped quantising. Both forms are asserted; the budget
  is the one with teeth.
- **(a, terrain) the day cycle never re-prepares a terrain material.** Four
  regions' materials, zero `Modified` events over the whole run, while the
  shared sky-lighting texture is re-uploaded repeatedly — the post-refactor
  statement of the defect that used to mark *every region's* material modified
  every frame. The zero proves itself: `touch_terrain_materials` writes the
  same four by hand and the next frame counts all four, so the channel is
  demonstrably live.
- **(b) the ambient is this frame's sky, not a function of the frames before
  it.** It is not rewritten inside a cell, and the value the app arrives at
  equals `sky_ambient_light` recomputed from scratch for the sky in force. A
  companion test pins `SkyPlugin`'s stated pre-sky zero against Bevy's 80-nit
  default. The *proportionality* half stays pure: `probe_ambient_scale` is a
  process-wide `OnceLock` over an environment variable and this workspace does
  not `set_var` in tests.
- **(c) a legacy sky reaches the tone mapper as `no_post`,** on every frame of
  the cycle, with a zero mix — and an EEP sky does not. The arithmetic was
  already pure-tested; what was untested is the three-system *wiring*,
  `drive_sky` → `ExposureRange` → `refresh_tonemap_settings`, which is the only
  part an app can speak to.
- **…and the sky still moves**, through each recorded channel, so the budget
  above cannot be met by a fixture that froze.

### Each assertion was watched to fail

A test that has never failed is a guess. Three regressions were injected in
turn and the suite re-run:

- the day-position quantiser removed → the budget test reports *40 of 40 frames
  wrote something*;
- `suppress_global_ambient`'s `PostUpdate` multiply put back → the ambient
  test fails (and the budget with it), at the idempotent `0.0` default, which
  is how the original defect hid;
- `drive_sky` publishing `can_auto_adjust: false` → the legacy-sky tone-mapper
  test fails.

Three items became crate-visible for the fixture, each still used by production
code so none becomes dead: `sky::DAY_POSITION_STEPS` (the step is expressed in
cells, which is the unit the assertions are in), `sky::sky_ambient_light`, and
`terrain::ensure_region`.

**Not addressed here:** the zero-test scene files this task listed as needing a
home. `water.rs`, `underwater_fog.rs` and `environment.rs` have since grown
tests of their own by other routes (13, 3 and 47); `glow.rs` still has none,
and its pass is not a function of the day cycle, so it wants a different
fixture rather than this one.
