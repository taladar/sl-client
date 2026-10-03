---
id: gridspec-local-chat
title: Local chat and typing on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-comms]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim drops out-of-range lines and marks every delivered one fully audible,
and relays typing without distance; the fake grid models ranges.

## Discover

The chat cases on both grids; audibility fading on SL, owner / region say, debug
channel visibility, cross-border shout, object `SourceType` (scripted prim on
each grid).

## Document

`book/src/gridspec/chat.md`.

## Fake grid

Small — per-flavour audibility / ranges in this task.

## Viewer

Partial audibility rendered if SL sends it.
