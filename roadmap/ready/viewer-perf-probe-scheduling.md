---
id: viewer-perf-probe-scheduling
title: Change-driven probe capture scheduling (zero idle cost)
topic: viewer
status: ready
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-perf-probe-instrumentation, viewer-perf-probe-filter-on-capture, viewer-perf-probe-capture-content, viewer-realtime-mirrors]
---

Context: [context/viewer.md](../context/viewer.md).

The headline item of the probe perf round. `CAPTURE_PERIOD_FRAMES = 180`
(`probes.rs`) was an emergency throttle, and merely shrinking it is not
the goal: the reference's model — **continuous** one-face-per-frame
churn, even with all its capture-time cost cuts — is explicitly NOT the
target, because it is what makes probes barely usable in Firestorm even
on top-range hardware. The target model is **change-driven with zero
idle cost**: a static scene captures nothing at all, and — with
[[viewer-perf-probe-filter-on-capture]] — pays no filter or blit either,
so the probe system's idle footprint is exactly zero.

Keep the existing base mechanism (≤ 1 face rendered per frame globally,
6-frame bursts per rig, the urgent queue for newly assigned rigs).
Replace the fixed period with dirty-driven burst starts:

- an object added / moved / changed within (or near) a probe's influence
  volume dirties that probe — hook the `apply_object` ingest flow;
- sky / day-cycle transitions dirty **all** probes (a windlight change
  must re-drive every cube);
- the default probe follows the viewpoint: camera movement past a
  distance threshold dirties it, as does (rate-limited) object churn
  within its draw distance;
- avatar / particle movement dirties only DYNAMIC probes — meaningful
  once [[viewer-perf-probe-capture-content]]'s layer split lands (soft
  ordering, not a hard blocker: without it, treat avatar churn as
  dirtying nothing rather than everything).

Among dirty rigs, pick the next burst reference-style:
`update_score = staleness − 0.1 × distance-to-camera`, never-captured
rigs first, an anti-starvation pick every third burst. Add a
frame-budget controller: when frame time exceeds a target (fed from
[[viewer-perf-probe-instrumentation]]), widen the minimum inter-burst
idle — **staleness, not FPS, absorbs overload**. A very slow background
sanity refresh (configurable, off-able) remains as the catch-all for
invalidations the triggers miss (e.g. textures finishing streaming on
distant geometry).

This is also the perf foundation for [[viewer-realtime-mirrors]]: a hero
probe is just a scheduler client pinned to every-frame cadence, budgeted
like everything else.

Acceptance: zero captures over N idle frames in a static gallery scene
(counter); a scripted object move or day-cycle change shows in the
mirror test sphere within ~0.5 s at FPS equal to today's 180-frame
baseline; an artificial GPU load makes the controller back off (capture
rate drops, frame budget held); staleness of every dirty rig stays
bounded (no starvation).

## Audit 2026-10-15 — the baseline moved; reconcile with the cadence lesson

`CAPTURE_PERIOD_FRAMES` is gone. `sl-viewer-world-scene/src/probes.rs` now
runs the reference's model: one face per frame scene-wide in six-frame
bursts, the local pool on a **continuous** oldest-first, distance-weighted
round-robin (`age − distance × PROBE_DISTANCE_WEIGHT`, 0.1), the
environment-only default probe every `DEFAULT_PROBE_PERIOD_SECS` (2 s), and
the `urgent` queue for a newly assigned rig. That is the continuous churn
this item says is not the target.

Before building the change-driven scheduler, resolve the conflict with what
was measured since: slowing probe cycling made Bevy purge the idle capture
views' pipelines / bind groups / specialization caches, so every late
capture paid a recalculation spike (commit `89c646ce`;
[[viewer-perf-probe-material-respecialize]] describes the per-view purge).
A scheduler that captures nothing when idle hits exactly that purge on its
next capture, so it needs the purge exemption (the upstream `bevy_pbr`
change [[viewer-perf-probe-material-respecialize]] proposes) or some other
way of keeping a parked view's caches warm — otherwise "zero idle cost"
trades a steady cost for spikes.
