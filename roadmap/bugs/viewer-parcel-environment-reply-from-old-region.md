---
id: viewer-parcel-environment-reply-from-old-region
title: A parcel environment reply from the old region may match by local id
topic: viewer
status: bugs
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
