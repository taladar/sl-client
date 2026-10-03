---
id: gridspec-lsl-throttles
title: Forced delays and throttles on each grid
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-call-cost-sizes]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Forced delays come from the documented `sleep` attributes, never measured;
OpenSim scales them by `ScriptDelayFactor`.

## Discover

Real delays for `llInstantMessage`, `llGiveInventory`, `llRezObject`; rez, HTTP,
`llCastRay`, region-say throttles (timing probes per the frame rules).

## Document

`book/src/gridspec/lsl.md` § Throttles.

## Fake grid

Feeds each library tranche.

## Viewer

None.
