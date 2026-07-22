---
id: viewer-perf-frame-churn-cleanups
title: Small per-frame churn cleanups (throttles + scratch reuse)
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

Grab-bag of small, individually-low-impact per-frame costs from the
2026-07-22 survey — each cheap to fix, together removing a steady
background of allocations, sorts, and redundant writes. Each item is
independent; land opportunistically.

- **Local-light ranking** — `drive_local_lights` (`lights.rs:286-363`):
  every frame collects all light prims into a fresh
  `Vec<(Entity, f32)>` (`:300`), computes luminance+distance,
  `sort_unstable_by` (`:315`), and builds a fresh `HashSet<Entity>`
  (`:332`) — while the actual ECS light refresh below is already
  change-gated (`:350-361`). The selection barely changes frame to
  frame. Fix: throttle to ~4 Hz like `drive_render_priority`
  (`render_priority.rs:65`) and keep the Vec/HashSet as `Local` scratch.
- **Status-bar FPS/clock** — `update_status_readouts`
  (`status_bar.rs:293-302`, body `540-578`): recomputes six readout
  strings at 60 Hz; the FPS string differs every frame, so it re-shapes
  every frame, and the clock recomputes 60× for a 1 Hz change. Fix,
  reference-faithful (verified in Firestorm): refresh the readout on a
  1 s timer (`llstatusbar.cpp:606`) and display the **median**
  per-second FPS over a 200-period ring advanced once per frame
  (`llstatusbar.cpp:621`, `lltracerecording.cpp:922`,
  `llappviewer.cpp:1631`) — that median-of-~200-frames is why the
  reference's FPS reads steadily instead of flickering. Keep the ring
  update itself trivial (push one float per frame); only formatting +
  shaping drop to 1 Hz. Same ring can later feed
  [[viewer-statistics-floater]].
- **Name-tag placement** — `position_name_tags`
  (`avatars.rs:3788-3823`): writes `node.left`/`node.top` every frame
  per tag → per-frame taffy relayout. Mostly inherent (screen-space
  tracking), but skip the write when the projected position is unchanged
  (sub-pixel delta) — idle camera + idle avatars then cost nothing.
- **Render-priority scratch** — `drive_render_priority`
  (`render_priority.rs:144`): allocates 5 `HashMap`/`HashSet` per run.
  Already throttled to 4 Hz, so minor — reuse `Local` scratch maps.
- **Particle-system clone** — `particles.rs:1029`: per source per frame
  clones the whole `ParticleSystem` struct into the ops snapshot. Borrow
  it, or clone only when the system parameters changed. (The big
  particle win is [[viewer-perf-gpu-particles]]; this quick fix is
  independent and should land first.)

## Estimated impact

Low individually; collectively removes a constant per-frame allocation
and relayout hum (most visible in Tracy memory mode as recurring small
allocations, and as steadier frame pacing on idle scenes). Good
first-contact tasks for the [[viewer-profiling]] workflow — each item is
a before/after measurement exercise in miniature.

Confidence: high — all call sites and existing throttles/gates verified.
