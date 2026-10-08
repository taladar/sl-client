---
id: gridspec-seated-crossing
title: A seated crossing on each grid — a vehicle carrying its riders over a border
topic: gridspec
status: ready
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
