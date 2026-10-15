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

Audit 2026-10-15: the stale read is not visible in the code. The fake grid's
`RequestRegionInfo` arm (`sl-fake-grid/src/parcel_edits.rs`) answers from
`SceneFixtures::limits` (`sl-fake-grid/src/world.rs`), which returns the
edited limits once an estate manager has changed them (since 2026-09-05), and
an edit is pushed as `RegionChange::RegionConfigured`. Pin it with a test
(edit, then request) before fixing anything; if it holds, only the flavour
defaults remain for the fake grid here.

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
