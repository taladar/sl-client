---
id: gridspec-simulator-features
title: The full SimulatorFeatures map on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-opensim-region-extras-limits]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

The extras block is OpenSim-only, `VoiceServerType` SL-only; OpenSim sends
`ExportSupported` as the string `"true"`. The aditi run was deferred; the
fake grid sends four keys.

## Discover

Make `simulator-features` record the whole map; run on aditi and OpenSim; diff.

## Document

`book/src/gridspec/region-arrival.md` § SimulatorFeatures (full per-grid key
table).

## Fake grid

Small — fill each flavour's map with the measured keys and value types.

## Viewer

Limits read per grid, `None` vs `Some(false)`, string booleans; check every
consumer of a key.
