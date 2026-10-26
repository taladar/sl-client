---
id: gridspec-lsl-land-region-env
title: LSL library behaviour on each grid — parcel, region, estate and environment queries
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-land-region-env]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: `llGetEnv` keys per grid, parcel details, region queries. Run on
OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Parcel, region, estate and environment queries.

## Fake grid

Large — [[server-lsl-lib-land-region-env]].

## Viewer

None.

## Done (2026-10-26)

- **Discover.** Six probes, `sl-lsl-runtime/probes/land/`: `env` (every
  `llGetEnv` key either grid is known to answer, a few neither documents, a
  key in capitals, the empty key, the frame rate the frame number counts);
  `parcel` (flags bit by bit, `llGetParcelDetails` code by code and together,
  prim limits, counts and owners, the land owner, `llOverMyLand`,
  `llScriptDanger`, `llParcelMediaQuery`, the music URL set (put back if it
  changed), and six scripts of one call each at a position outside the
  region); `region`
  (corner, flags, load, host, `llGetAgentList` per scope, `llEdgeOfWorld`,
  `llRequestSimulatorData` per code and name); `ground` (`llGround`,
  `llWater` at and beyond the edges, normal, slope and contour, wind, cloud);
  `day-cycle` (day length and offset, time of day, sun and moon directions
  and rotations, parcel against region); and `sl-only` (`llGetSimStats`,
  `llGetEnvironment` per rule, `llGetRegionTimeOfDay`,
  `llGetClosestNavPoint`). Complete on aditi and both OpenSim engines, which
  agree on every line but a dwell; `sl-only` does not compile on OpenSim,
  which is its measurement.
- **Found on the way.** OpenSim's `llGetParcelFlags` outside the region
  throws and stops the script, which is why the outside calls are scripts of
  their own; XEngine cannot parse `<…> - v`. Both are notes in
  `book/src/tools/lsl-probes.md`.
- **Not measured.** The write half on land the owner owns — music URL, parcel
  media commands, pass and ban lists, `llEjectFromLand`,
  `llManageEstateAccess` — since our aditi avatars own no land
  ([[gridspec-aditi-test-land]]) and a change to the local parcel's access
  lists would outlive the run; the damage and combat functions, which the
  library tranche lists as out of scope; `llGetRegionTimeDilation` under
  load.
- **Document.** `book/src/gridspec/lsl.md` § Parcel, region, estate and
  environment queries.
- **Fake grid.** Large, as planned: the rows are in
  [[server-lsl-lib-land-region-env]].
- **Viewer.** None.
