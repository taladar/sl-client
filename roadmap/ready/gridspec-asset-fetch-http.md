---
id: gridspec-asset-fetch-http
title: HTTP asset fetch details on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Fetches are green on both grids; ViewerAsset 503s happen on aditi; asset caps
live at the root region on SL.

## Discover

Extend the fetch cases to record status codes, ranges (206 vs 200), redirects,
cache headers, missing-asset code.

## Document

`book/src/gridspec/assets.md`.

## Fake grid

Small — status-code and range fidelity per flavour.

## Viewer

Retry on 503, partial ranges.
