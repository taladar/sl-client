---
id: gridspec-object-rez-derez
title: Rez, take, take-copy, delete, return and auto-return on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-conformance-object-asset-format-fails-under-load]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`object_assets` / `inventory_announcement` rows exist. On stock OpenSim
`ObjectDelete` is a no-op and a Delete derez resolves the caller's Trash; the
fake grid honours `ObjectDelete` on both flavours.

## Discover

Run `object-rez-derez` on aditi (sandbox debris!); record `ObjectDelete`,
take-copy, no-rez land refusal, temp-on-rez timing.

## Document

`book/src/gridspec/building.md` § Rez and take.

## Fake grid

Small — `ObjectDelete` flavour row, rez refusal.

## Viewer

Take waits for either announcement; the no-copy Restore guard.
