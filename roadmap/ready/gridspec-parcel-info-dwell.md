---
id: gridspec-parcel-info-dwell
title: Parcel info, dwell and the remote parcel id on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-parcel-info-reply-flags-misread]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Green on OpenSim (dwell 0); OpenSim's flags byte packing known from source; SL's
unmeasured.

## Discover

`parcel-info-dwell` on aditi recording the flags byte of a for-sale and a
group-owned parcel.

## Document

`book/src/gridspec/land.md` § Parcel info.

## Fake grid

Small — the flags byte per flavour.

## Viewer

The decoded flags (see the bug).
