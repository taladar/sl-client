---
id: gridspec-seated-crossing
title: A seated crossing on each grid — a vehicle carrying its riders over a border
topic: gridspec
status: done
origin: gridspec-neighbours-crossing (2026-10-07)
refs: [gridspec-neighbours-crossing, viewer-seated-region-crossing,
  test-handover-distant-and-vehicle-aditi, test-fake-grid-seated-crossing,
  server-world-sit-and-attach]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

A crossing on foot or by flight is measured
(`book/src/gridspec/teleport.md`, *Neighbours and crossings*): both grids
send a `CrossedRegion` over the event queue, neither kills the avatar on the
region it left, and the destination re-sends the avatar under a new local id.
The fake grid hands a ridden vehicle over — killed in one region, rezzed in
the next under new local ids, its riders re-parented
([[test-fake-grid-seated-crossing]]) — on OpenSim's source and on nothing
measured. Second Life's unsit and resit at a vehicle crossing is assumed.

## Discover

- A moving seat on each grid: a prim the avatar rezzes, a script uploaded
  into it that sets a sit target and steps the prim over the border once
  somebody sits (`llSetPos` for a non-physical one), and the avatar seated
  on it. The script has to get into the prim by upload on both grids, as
  `sit-stand` does it; a seat with a sit target is answered on aditi from
  any distance tried, a scriptless one only from within eight metres or so
  ([[gridspec-sit-stand]]).
- Record, with `sl_conformance::crossing`: the `CrossedRegion` and what
  comes with it, the vehicle's `KillObject` on the region left and its
  update on the one entered (ids, order, timing against the avatar's), the
  avatar's `ParentID` through it, whether an unsit and a resit are visible,
  and what becomes of the sit-implied script permissions
  ([[viewer-seated-region-crossing]]).
- A second rider on the same vehicle, seen from the first.

## Document

`book/src/gridspec/teleport.md` § Neighbours and crossings.

## Fake grid

The ridden hand-over exists; give each flavour the measured order and
timing.

## Viewer

Seat survival and placement across the border
([[viewer-seated-region-crossing]]).

## Done (2026-10-28)

**Discover.** New conformance case `seated-crossing`
(`sl-conformance/src/cases/seated_crossing.rs`, OpenSim and aditi, two
avatars): a cube rezzed beside the nearest border a neighbour shares, a
script uploaded into it (sit target, sit-implied permissions, two controls
taken, and `go` / `push` on chat channel 7311 to step it over with
`llSetPos` or push it physical with `llSetVelocity`), both avatars seated, and
both sessions watched at once (`CrossedRegion`, the vehicle's copies, each
rider's `ParentID` trail, the other rider, the script's own reports, control
changes, sit responses, permission requests). `sit_stand::plant_script` is the
script-planting helper, generalised from `plant_sit_target`.

- OpenSim: four runs, all crossed (three stepped, one pushed after a step
  landed exactly on x = 256.0 and stuck there — the step is now 1.9 m).
- aditi: two runs from a sandbox parcel in Morris (10/100/35, the user's
  find) into Bonifacio, both crossed. Ahern refused the rez; Mauve has no
  neighbours. The first Morris runs never saw their cube: a session's camera
  sits at the region centre until set, and Second Life streams by the camera,
  so in a busy sandbox the new cube was not sent within 30 s. The case aims
  both cameras at the seat, and at the landing spot after the crossing.

**Document.** `book/src/gridspec/teleport.md` § A seated crossing (replaces
the "not measured" paragraph); `book/src/conformance/runner.md` and
`book/src/tools/fake-grid.md`.

**Fake grid.** `FakeGrid::cross_vehicle` + `RiddenCrossing` replace the test
helper. Both flavours now hand the sit to the destination ahead of the
arrival (the arrival burst sends the seat, then the agent seated on it) and
send no kill from the region left; `NeighbourViewPolicy::vehicle_handover`
says when the vehicle comes (`VehicleFirst`, OpenSim: down the child circuit
before the `CrossedRegion`; `WithArrival`, Second Life: with the arrival
burst). The unsit-and-resit order the grid assumed for Second Life before is
gone — neither grid does it.
`on_opensim_the_vehicle_crosses_ahead_of_its_riders` and
`on_second_life_the_vehicle_arrives_with_its_riders` hold each order (both
verified by mutation). The case itself does not run offline: the fake grid
runs no scripts.

**Viewer.** The full-stack ridden-crossing test runs once per flavour
(`an_open_sim_rider_stays_on_its_vehicle_across_a_border`), through the new
`ViewerHarness::ride_to`. Findings for the permission question noted on
[[viewer-seated-region-crossing]].
