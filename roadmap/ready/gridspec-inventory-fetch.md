---
id: gridspec-inventory-fetch
title: Inventory skeleton, fetch caps, AIS3, the library and cache versions on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-ais3-nested-embedded,
  protocol-ais3-library-cap, protocol-fetch-inventory-items-request]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

SL drops the UDP descendents fetch silently (the fake grid refuses), nests
AIS3 `_embedded` per depth (our parser reads one level); OpenSim's library
leaves carry a phantom nil folder. The aditi runs of the fetch / crawl /
library / cache cases never happened.

## Discover

Those cases on aditi plus an AIS3 depth probe and an item-fetch probe; skeleton
fields per option; whether versions advance on UDP / other-session changes.

## Document

`book/src/gridspec/inventory.md` § Fetching.

## Fake grid

Small — nested `_embedded` per flavour, OpenSim's phantom folder, version bumps.

## Viewer

No UDP on SL, nested parser, `LibraryAPIv3` routing.
