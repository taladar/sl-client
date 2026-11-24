---
id: viewer-parcel-environment-reply-from-old-region
title: A parcel environment reply from the old region may match by local id
topic: viewer
status: done
origin: mixed-channel race audit (2026-11-23)
refs: [viewer-environment-not-requested-after-region-change]
---

Context: [context/viewer.md](../context/viewer.md).

Plausible and not reproduced.

## Bad order

A parcel environment HTTP request is in flight when the agent changes region,
and the reply lands after the new region's parcel is known.

## Cause

`ingest_parcel` (`sl-viewer-world-scene/src/environment.rs`) compares only the
region-local parcel id, and replies carry no region.

## Effect

The wrong parcel sky shows until the next parcel change.

## Fix

Stamp environment requests with a region generation and drop replies from an
older one.

## Fix (2026-11-24)

No generation stamp was needed. Every `ExtEnvironment` reply names its region
(`region_id`) on both grids, as Firestorm's `recordEnvironment` relies on. The
session now records each circuit's region id from its `RegionHandshake` and
drops an environment reply naming a region other than the root's. This covers
region-scoped replies too, which could otherwise end the new region's retry
loop with the old region's sky. A reply naming no region is kept.

A distant teleport cleared the world cache when the handover committed,
including the destination's handshake facts. The new root's region flags and id
now survive that clear, so `region_blocks_fly` is known straight after a
distant teleport as well. `SimSession` stamps its own region id on every
environment it serves, so the fake grid never serves settings recorded under
another region's id.
