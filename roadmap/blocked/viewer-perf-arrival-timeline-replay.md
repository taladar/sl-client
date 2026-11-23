---
id: viewer-perf-arrival-timeline-replay
title: Replay a captured arrival timeline to measure streaming spikes
topic: viewer
status: blocked
origin: Region capture design discussion (2026-11-22)
blocked_by: [viewer-perf-region-capture-replay]
refs: [viewer-perf-asset-streaming-frame-spikes]
---

Context: [context/viewer.md](../context/viewer.md).

## Goal

Measure what happens when a lot of data lands at once, repeatably. A settled
scene cannot show these spikes, and a live session never delivers the same
batches twice.

## Pacing modes

- **Frame-locked:** the same events on the same frame number every run. This is
  the before/after measure, because each build gets identical batches.
- **Clock-locked:** events at their recorded time offsets, so a slow frame
  collects a larger batch. This checks that an improvement holds under realistic
  timing. A faster build sees smaller batches here, so it is not a clean A/B.
- **Synthetic bursts:** a chosen number of objects or decoded textures in one
  frame, to find where the viewer breaks down.

## What needs care

- Replay must hold each asset back until its recorded arrival. A local cache
  would otherwise deliver everything at once.
- Budgeted systems can flatten a spike while the scene takes far longer to
  load. Every run reports the worst frame, p99, time until settled and, where
  it can be exposed, the peak backlog.
- Decode and mesh tasks finish on worker threads in varying order. Keep them
  real and compare distributions over several runs.

The recording covers everything from session events onward. UDP parsing and
HTTP are outside it.
