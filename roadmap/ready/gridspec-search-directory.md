---
id: gridspec-search-directory
title: Directory search and the avatar picker on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-classified-query-wrong-flag-space,
  viewer-search-maturity-filter]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim answers only people / groups; SL's UDP picker returns a nil sentinel;
web search URL sources differ; classified search is broken by our flag bug.

## Discover

The directory cases on aditi (after the classified flag fix) and OpenSim;
paging, maturity filtering.

## Document

`book/src/gridspec/search.md`.

## Fake grid

Medium — [[server-fake-grid-directory-search]].

## Viewer

Sentinel row, empty tabs on OpenSim.

## Capabilities done in this task

[[protocol-cap-product-info]]: land-type names over `ProductInfoRequest`, with
the `ProductSKU` the LLSD forms of the land replies carry.

## Already known (from [[gridspec-parcel-info-dwell]], 2026-10-05)

Aditi answers `DirLandQuery` over the **event queue**: a `DirLandReply` event
mirroring the UDP blocks, each row with a `ProductSKU` (`023`, `024` seen). The
session decodes it into `Event::DirLandReply` (without the SKU) since that
task; before it the event was dropped, which is why `dir-places-land-classified`
recorded no land answer on Second Life. Still to measure here: paging, the
other searches' transports, and whether `PlacesReply` comes the same way.
