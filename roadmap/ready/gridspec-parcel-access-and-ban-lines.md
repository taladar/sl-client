---
id: gridspec-parcel-access-and-ban-lines
title: Parcel access and ban lists, enforcement and ban lines on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-parcel-ban-line-display,
  viewer-parcel-ban-duration, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Green on OpenSim (fresh transaction id per update, nil placeholder for empty);
enforcement and ban-line pushes are read from OpenSim source only; nothing
enforced on the fake grid.

## Discover

`parcel-access-list` plus a second avatar walking into a banned parcel (OpenSim
as estate owner); on aditi needs owned land.

## Document

`book/src/gridspec/land.md` § Access.

## Fake grid

Small list handling in this task; enforcement —
[[server-fake-grid-parcel-access-enforcement]].

## Viewer

Ban lines drawn, overlay redrawn, multi-part lists.
