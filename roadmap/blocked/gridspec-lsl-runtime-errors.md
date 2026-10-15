---
id: gridspec-lsl-runtime-errors
title: LSL run-time errors on OpenSim and the debug channel in our viewer
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-runtime-errors,
  viewer-script-warning-window]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

[[server-lsl-runtime-errors]] (in progress) carries the aditi probe; OpenSim's
texts include the C# exception. Debug-channel chat now reaches the viewer's
Nearby transcript as kind `debug` (`sl-viewer-people/src/conversations.rs`,
`chat_kind`; pinned by `e2e_pilot.rs`'s debug-channel test), but there is no
script error / debug window yet ([[viewer-script-warning-window]]).

## Discover

The OpenSim column of every run-time error; list bounds; stack-heap wording;
Mono vs LSO format.

## Document

`book/src/gridspec/lsl.md` § Run-time errors.

## Fake grid

Within [[server-lsl-runtime-errors]] per flavour.

## Viewer

Merge the two script error window tasks; route the debug channel into that
window; live check with a division by zero.
