---
id: viewer-perf-probe-irradiance-split
title: Separate low-res irradiance path for probes (reference-style 16 px)
topic: viewer
status: ideas
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-perf-probe-filter-on-capture, viewer-p33-3]
---

Context: [context/viewer.md](../context/viewer.md).

The reference convolves irradiance into a tiny separate 16 px cube array
while radiance gets the mipped GGX chain; Bevy's
`GeneratedEnvironmentMapLight` produces both halves from the full-res
cube in one chain, with one `intensity` across both. A split path would
shrink the diffuse half of the filter cost — and is the door to
modelling the probe **ambiance** parameter, which P33 deliberately left
unmodelled ([[viewer-p33-3]]) precisely because Bevy cannot scale the
irradiance half without dragging the reflection with it.

Deep `bevy_pbr` surgery and upstream-shaped (it changes what the filter
produces and what the shader binds). Only worth costing after
[[viewer-perf-probe-filter-on-capture]] shows how much filter cost
remains once the every-frame refilter is gone — if filtering is by then
a rare per-capture event, the diffuse half may be too cheap to bother.
