---
id: viewer-audit-probe-ambient-multiply
title: suppress_global_ambient multiplies an absolute producer and decays it geometrically
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-scene-change-guards-day-cycle]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/probes.rs:201` — `suppress_global_ambient` runs
`ambient.brightness *= probe_ambient_scale();` in `PostUpdate`, against a
producer that writes an **absolute** value: `sky.rs:787` compares `to_bits()`
specifically to avoid dirtying `GlobalAmbientLight`.

The product is never what the sky asked for, so the sky rewrites every frame and
the resource is dirty every frame regardless of the guard. And `drive_sky`
early-returns when no sky frame resolves (`sky.rs:711`) — while it does, the
multiply runs unopposed and ambient decays geometrically toward zero.

This is currently correct **only by accident**: `probe_ambient_scale()` defaults
to exactly `0.0` (`:193`), which is idempotent. Set the documented
`SL_VIEWER_PROBE_AMBIENT_SCALE=0.5` A/B knob and ambient decays to nothing over
a few frames.

Fix: apply the suppression as part of computing the absolute ambient value, not
as a multiplicative post-pass. One of the three "right under the harness, wrong
on a live grid" scene defects — see
[[viewer-audit-scene-change-guards-day-cycle]].

## Fixed (2026-08-28)

`suppress_global_ambient` is gone. `probe_ambient_scale` is now a
**factor of the value a producer asks for** rather than an attenuation applied
to the resource afterwards, and every producer of a flat ambient applies it
itself: `drive_sky` folds it into the brightness it writes (via a pure
`sky_ambient_light(ambient, probe_scale)` that carries the luminance / hue /
`AMBIENT_BRIGHTNESS_SCALE` arithmetic the system used to do inline), and the
gallery's `hold_stage_ambient` scales its stage constant by the same call.

Both halves of the defect go with it. There is no longer a second writer to
compare against, so `drive_sky`'s `to_bits()` guard holds and
`GlobalAmbientLight` is dirty only when the sky frame actually changes; and
nothing runs on the frames `drive_sky` early-returns on, so a region with no
resolved sky holds the last ambient instead of decaying toward zero. The knob is
now what it says it is: `SL_VIEWER_PROBE_AMBIENT_SCALE=0.5` gives half the sky's
ambient, every frame, instead of half of the previous frame's.

### Three ambients were silently zero, and had to stay that way

The multiply ran in `PostUpdate` with no ordering against extraction, so at the
default scale of `0.0` it zeroed `GlobalAmbientLight` on **every** frame of
**every** app that added `ReflectionProbePlugin` — whatever anyone else had
written earlier in the frame. Removing it therefore un-hides three values that
have never once reached a shader, and each had to be re-stated deliberately
rather than allowed to switch on:

- The **gallery** wrote `STAGE_AMBIENT` (200 nits) each `Update` and had it
  zeroed each `PostUpdate`; its doc comment, and a note in
  [[viewer-render-scene-coverage]], both describe a 12-vs-200-nit fill that was
  in fact 0-vs-0. It now multiplies by `probe_ambient_scale` like the sky, so
  the rendering is unchanged at the default and the constants become live under
  the knob — the gallery runs the viewer's real probes, and a flat fill stacked
  on their image-based ambient is exactly the double-count P33.3 calibrated
  away. `setup_stage`'s second, unscaled `insert_resource` of the same resource
  went; `hold_stage_ambient` is the sole writer, and compares before writing (it
  wrote unconditionally before **because** it was racing the multiply).
- The **readback harness** set nothing and inherited Bevy's default 80 nits,
  also zeroed. It now states `brightness: 0.0`: every assertion there is about
  which side of a mirror a colour landed on, and 80 nits of fill washes out the
  contrast that decides it.
- The **viewer before its first sky frame** likewise inherited 80 nits.
  `SkyPlugin` now inserts `0.0` at build, so a world between login and its first
  `EnvironmentSettings` does not flash a flat fill that `drive_sky` then
  removes.

`probe_ambient_scale` became `pub` for the gallery (it is in another crate); the
module docs in `probes.rs`, `drive_sky`'s "writes nothing under a static
environment" claim, `context/viewer.md` and
[[viewer-perf-probe-quality-knobs]]'s "bypass" note were all rewritten to
describe the factor rather than the post-pass.

### Tests

Four in `sky.rs`, all on the pure `sky_ambient_light`: the share is proportional
(half the scale is half the brightness, `0.0` is none); a frame's ambient is
**bit-identical** across 120 frames while the post-pass this replaced would have
decayed by `0.5^120`, which is the regression and the guard's own comparison;
the tint is the sky's normalised hue whatever the share; and a black sky stays
finite rather than dividing by its own zero peak.

That covers (b) of [[viewer-audit-scene-live-daycycle-fixture]] — "ambient
converges rather than decaying under a non-zero `probe_ambient_scale`" — at the
pure level. The fixture stays open for the app-level version and for (c).
