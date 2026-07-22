---
id: viewer-realtime-mirrors
title: Real-time mirrors (hero probes)
topic: viewer
status: blocked
origin: render-feature gap analysis vs Firestorm (2026-07)
blocked_by: [viewer-perf-probe-scheduling]
---

Context: [context/viewer.md](../context/viewer.md).

Actual mirrors — a surface that reflects the scene (and *you*) in real time,
re-rendered every frame. SL added this as the **"hero probe"**: a reflection
probe that, unlike the static/blurry P33 probes, is rendered fresh per frame
from the mirror plane, so it is sharp and live. It is what makes a bathroom
mirror or a shop mirror-wall work.

Firestorm gates it on `RenderMirrors`, with `RenderHeroProbeResolution`
(sharpness) and `RenderHeroProbeUpdateRate` (how often it re-renders — the perf
lever).

Scope: identify mirror surfaces (the material/flag that marks a face as a hero
reflector), render the scene from the reflected camera into the probe target
each frame (or every N frames per the update rate), and sample it on the mirror
face. This is expensive — a second scene render per active mirror — so the
instance cap and update-rate throttle are part of the feature, not an
afterthought.

Reference (Firestorm, read-only): the hero-probe path, `RenderMirrors`,
`RenderHeroProbe*`.

Builds on: the P33 reflection-probe infrastructure (this is its dynamic,
per-frame cousin). Blocked on [[viewer-perf-probe-scheduling]]: a hero
probe is a scheduler client pinned to every-frame cadence, and on
today's capture cost structure a per-frame six-face render would tank
the frame rate — the change-driven scheduler and the cheap-capture work
it builds on are this feature's perf foundation.
