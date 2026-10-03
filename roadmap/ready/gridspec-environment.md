---
id: gridspec-environment
title: Region and parcel environments (EEP) on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-e2e-environment-parcel-layer,
  viewer-live-opensim-world-renders-white]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim's default day: 14400 s, offset 57600, 8 sky frames over 4 tracks + 1
water. Aditi's `ExtEnvironment` cap arrives late; per-track `trackno` path;
SL sun HDR scale. The fake grid has no parcel environments.

## Discover

Extend `environment` to dump frames on both grids; parcel overrides as OpenSim
estate owner; `Probe::Environment` in `e2e_environment` live.

## Document

`book/src/gridspec/environment.md`.

## Fake grid

Small — per-flavour default day and cap timing; parcel environments are
[[server-fake-grid-parcel-on-movement]].

## Viewer

Late cap, `trackno`, HDR scale, parcel layer.
