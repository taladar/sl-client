---
id: gridspec-region-info
title: RegionInfo, region flags and limits on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-region-flag-deny-ageunverified-value,
  viewer-script-limits]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`RegionInfo5` is newer-SL-only; `OpenRegionInfo` OpenSim-only; the fake grid
answers `RequestRegionInfo` from defaults, not its store (stale-read bug).

## Discover

Extend `region-info` to record every block and field on both grids (no rights
needed).

## Document

`book/src/gridspec/estate.md` § Region info.

## Fake grid

Small — flavour defaults and the stale read in this task.

## Viewer

Absent blocks, `u64` flags, status-bar icons.

## Capabilities done in this task

[[protocol-cap-dispatch-region-info]]: region settings over
`DispatchRegionInfo` (Second Life), including the two flags UDP cannot
carry. Also the protocol half of `RegionSchedule` (read and write the restart
schedule; the floater is [[viewer-region-restart-schedule]]).
