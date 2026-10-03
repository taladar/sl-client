---
id: gridspec-object-media
title: Media on a prim: ObjectMedia, navigation and propagation on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-media-prim-browser]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

The fake grid stores ObjectMedia per session and never bumps `MediaURL`; nothing
is measured live.

## Discover

An `object-media` case: set, navigate, version string format, propagation to a
second avatar, permission refusal, whitelist; both grids.

## Document

`book/src/gridspec/media.md` § Media on a prim.

## Fake grid

Large — [[server-fake-grid-object-media-region]].

## Viewer

Refetch on version change.
