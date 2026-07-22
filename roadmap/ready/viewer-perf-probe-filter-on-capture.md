---
id: viewer-perf-probe-filter-on-capture
title: Filter probe cubemaps only on capture completion; blit only dirty faces
topic: viewer
status: ready
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-perf-probe-instrumentation, viewer-p33-1, viewer-p33-3]
---

Context: [context/viewer.md](../context/viewer.md).

The probe system's single largest **standing** cost — paid every frame
even when no capture happens: `bevy_pbr`'s `GeneratedEnvironmentMapLight`
re-runs its full SPD-downsample + GGX-radiance + Lambertian-irradiance
compute chain **every frame for every live probe** (bevy_pbr-0.19
`light_probe/generate.rs` has no dirty check) — up to 5 chains per frame
while at most one cube face changed. Smaller organ, same disease:
`copy_probe_faces` (`probes.rs`) copies all 6 faces of every live rig
into its cube every frame, changed or not. The reference filters only
when a capture completes (face 5 of 6). Goal: **zero probe GPU cost on a
frame with no capture activity** — the precondition for making captures
more frequent ([[viewer-perf-probe-scheduling]]).

Both halves hang off one dirty signal: which (rig, face) the
`CaptureSchedule` rendered this frame (the resource is already extracted
to the render world for the blit).

- **Blit** (in-tree, easy): copy only the face rendered this frame.
- **Filter** (the decision this task makes first): run a rig's filter
  chain only on the frame(s) after its burst completes.
  1. Preferred: fork `bevy_pbr` under `[patch.crates-io]`, add an update
     mode to `GeneratedEnvironmentMapLight` (e.g.
     `EveryFrame` / `OnChange`, keyed off the source image's change ticks
     or an explicit dirty flag), gate the generation node on it, and
     submit the feature upstream — real-time-capture users all want this
     (the fork-upstream-for-upstream-gaps convention).
  2. Fallback: component cycling — insert the generator component only
     for the completion frame, then keep a plain `EnvironmentMapLight` on
     the filtered outputs. Needs a spike first: `generate.rs`
     auto-inserts the `EnvironmentMapLight` with GPU-filled placeholder
     images, so verify the filtered outputs survive generator removal,
     and that `light_capture_cameras` (which shares the filtered handles
     into the capture cameras for bounce feedback) keeps working.

Note for the doc comment: per-completion filtering makes the bounce
feedback converge per capture cycle instead of per frame — which is
exactly the reference's two-pass model, and fine.

Acceptance (via [[viewer-perf-probe-instrumentation]] counters): 0 filter
chains and 0 face blits on frames without capture activity; GPU frame
time at 5 live probes drops by about the measured 5-chain cost. Probe
lighting output itself unchanged — only the cadence changes: render
goldens within tolerance, `SL_VIEWER_PROBE_TEST_SPHERE=1` mirror ball
still matches the scene beside it.
