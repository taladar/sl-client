---
id: gridspec-environment
title: Region and parcel environments (EEP) on each grid
topic: gridspec
status: done
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

## Done (2026-10-05)

- **Discover.** `environment` records the whole region reply, the reply for a
  parcel that only inherits, what a set without rights is told (both grids),
  and — as OpenSim's estate owner — a parcel set and reset, with the estate's
  parcel-environment switch turned on for the measurement and put back. A
  headless viewer on each grid was read with `sl-viewer-ctl environment`.
- **Measured** (`book/src/gridspec/environment.md`). Both defaults: 14400 s,
  offset 57600 s, eight ground-track sky keyframes, one water. A parcel that
  inherits is answered `is_default` with no day, on both. OpenSim answers a set
  with a bare `{success: true}` and a reset with the ids and `success`, hides a
  stored parcel environment while the estate disallows them, and refuses a
  single-track set. Refusals carry a reason on both.
- **Client.** A bare acceptance and a refusal were both logged as replies
  that did not parse, and nothing reached the caller. Now a bare acceptance is
  followed by a GET (both runtimes) and a refusal is
  `Event::EnvironmentChangeRefused`, shown as the `WLRegionApplyFail` alert.
  `EnvironmentUpdate` and `EstateFlags` are re-exported from the tokio client.
- **Fake grid.** The inheriting-parcel answer on both flavours, OpenSim's bare
  replies, the default day's name and version per flavour.

## Not done as written

- **Per-flavour default day.** Only its labels. Both grids' default is an
  eight-keyframe day; the fake grid's stays a single keyframe on purpose, so a
  render capture does not depend on the region clock.
- **Cap timing.** The capability was already granted at the handshake on both
  grids, so there is no per-grid timing to imitate; the viewer's retry stays.
- **Hiding a parcel environment while the estate disallows them** (OpenSim) is
  left to [[server-fake-grid-parcel-on-movement]], which gives the fake grid
  the estate switch it depends on.
