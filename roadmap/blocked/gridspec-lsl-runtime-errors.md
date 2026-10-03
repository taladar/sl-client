---
id: gridspec-lsl-runtime-errors
title: LSL run-time errors on OpenSim and the debug channel in our viewer
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-runtime-errors,
  viewer-script-warning-window, viewer-script-error-window]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

[[server-lsl-runtime-errors]] (in progress) carries the aditi probe; OpenSim's
texts include the C# exception; our viewer shows debug-channel chat nowhere.

## Discover

The OpenSim column of every run-time error; list bounds; stack-heap wording;
Mono vs LSO format.

## Document

`book/src/gridspec/lsl.md` § Run-time errors.

## Fake grid

Within [[server-lsl-runtime-errors]] per flavour.

## Viewer

Merge the two script error window tasks; show the debug channel; live check with
a division by zero.
