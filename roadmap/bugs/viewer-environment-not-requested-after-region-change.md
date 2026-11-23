---
id: viewer-environment-not-requested-after-region-change
title: The region environment is not requested again after a crossing or teleport
topic: viewer
status: bugs
origin: mixed-channel race audit (2026-11-23)
refs: [viewer-parcel-environment-reply-from-old-region]
---

Context: [context/viewer.md](../context/viewer.md).

Confirmed by reading the code. Not reproduced live. This is not itself an
ordering race, but the fix has an ordering constraint.

## Problem

`request_environment` (`sl-viewer-world-scene/src/environment.rs`) starts a
fetch on `RegionHandshakeComplete` or `RegionLimits`. Its comment says the
handshake covers a border crossing, but `complete_arrival` emits
`RegionHandshakeComplete` for login only. Crossings and teleports emit
`RegionChanged`, which this file does not handle. `RegionInfo` is only asked
for by About Region.

## Effect

The old region's sky can stay until something else triggers a fetch. The reset
in the handshake branch is skipped too: pushed experience skies and the
remembered parcel and its environment survive the region change.

## Fix

Run the same reset and request on `RegionChanged`, but send the request only
after the new region's capability map has landed, as Firestorm does in
`llenvironment.cpp`. Otherwise the GET goes to the old region's URL.
