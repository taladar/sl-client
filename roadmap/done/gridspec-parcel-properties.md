---
id: gridspec-parcel-properties
title: ParcelProperties transport, fields and pushes on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Both grids answer over the event queue (the fake grid over UDP) and push on
arrival and parcel crossing; the fake grid's limits are OpenSim's even when
imitating SL.

## Discover

Extend `parcel-properties` to record transport and every field, run on aditi; a
walk-across-a-parcel-line probe.

## Document

`book/src/gridspec/land.md` § Parcel properties.

## Fake grid

Small in this task: event-queue transport, SL limits, the avatar-sound fields.
The movement push: [[server-fake-grid-parcel-on-movement]].

## Viewer

Both transports; current parcel tracked from pushes.

## Done (2026-10-05)

- **Discover.** `parcel-properties` records the whole decoded record, and the
  tokio client logs each raw event-queue body at trace level
  (`sl_client_tokio::wire=trace`), which is what the field tables were read
  from. New case `parcel-crossing` finds the nearest parcel line by querying
  outwards, flies over it and back, and records every pushed parcel (OpenSim:
  the estate owner divides the region first and joins it back).
- **Measured.** Same keys on both grids, six fields in different LLSD types;
  `ParcelExtendedFlags` on Second Life only. Both push on arrival (sequence 0)
  and on every crossing; Second Life's push ids count up for the session,
  OpenSim's are always 0.
- **Fake grid.** Parcel records already go over the event queue with the
  visibility fields ([[gridspec-parcel-management]]); each flavour now writes
  its grid's wire types (`ParcelLlsdDialect`).
- **Viewer.** About Land's request ids met Second Life's push ids (both small
  positive numbers), so a crossing could answer a window's question; they now
  count down from -100000. The agent's parcel is resolved from position and
  bitmap, not from the sequence id.
- **Limits.** Second Life's limits follow the region product (Openspace
  1000 land impact / 15 agents, Homestead 7500 / 20, Full Regions a range under
  one name), OpenSim's are 15000 / 40 for every region. The fake grid takes a
  region's budget from `ImitatedGrid::region_capacity` by
  `RegionConfig::product`, overridable per region (`RegionConfig::capacity`),
  shares it among the parcels by area and reports it in `RegionInfo`.

## Left to another task

- **The counting push id on the fake grid.** Measured for the arrival and the
  movement pushes only; the fake grid has no movement push yet, so the count is
  built with it in [[server-fake-grid-parcel-on-movement]] rather than guessed
  onto the pushes an edit sends other occupants.
