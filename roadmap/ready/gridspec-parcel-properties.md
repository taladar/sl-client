---
id: gridspec-parcel-properties
title: ParcelProperties transport, fields and pushes on each grid
topic: gridspec
status: ready
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
