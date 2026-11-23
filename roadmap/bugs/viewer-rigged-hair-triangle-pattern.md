---
id: viewer-rigged-hair-triangle-pattern
title: Rigged mesh hair shows a pattern of individual triangles
topic: viewer
status: bugs
origin: live Aditi session after the Bevy 0.20 upgrade (2026-11-22)
refs: [viewer-mesh-hair-not-rendering]
---

Context: [context/viewer.md](../context/viewer.md).

## Cause and fix

Triangle order. Rigged alpha writes depth, so a triangle drawn early hides the
hair layers behind it, even where its texture is nearly transparent. The
reference runs every mesh face through `meshopt_optimizeVertexCache`
(`LLVolumeFace::cacheOptimize`), and content is authored against that order.
Reversing our order made the holes far worse, and applying the reference's
removed them. `to_bevy_rigged_mesh` now does the same.

Replays of both captured hairs are clean from behind and from the side. A live
session has not confirmed it yet, so this stays open until one does.

## Observation

One avatar's long rigged mesh hair (39 linked prims, up to 8 faces each, two
textures) breaks up into individual triangles in this viewer. The same hair on
the same avatar is drawn correctly by Firestorm at the same moment. Firestorm
has occasionally shown a similar fault on other hair in the past, so the content
may be sensitive to something both viewers do.

A local capture reproduces it offline: replayed from behind or from the side,
pale triangles form a regular zig-zag over the upper half of the hair. Captures
hold other residents' assets, so they stay on the machine that made them.

## What has been changed

Each of these matches the reference, and they are in place:

- Rigged alpha writes depth, discarding fragments below the reference's
  `MINIMUM_ALPHA` (`LLDrawPoolAlpha::forwardRender`). This removed most of the
  broken triangles.
- Rigged alpha draws ahead of other alpha in object and face order, unsorted by
  distance (`LLVolumeGeometryManager::genDrawInfo`). No visible change here.
- Rigged meshes carry MikkTSpace tangents (`LLVolumeFace::cacheOptimize`). The
  pale triangles remain.

## What was ruled out

The hair's legacy materials were missing from the first capture and looked like
the cause. A second capture holds every asset of another affected avatar, and
it still showed the triangles until the order was fixed.

## A second capture

A local region recording from 2026-11-23 (see
[viewer-perf-region-capture-replay](../ready/viewer-perf-region-capture-replay.md))
includes a different avatar, wearing different hair, with the same fault.

## To find out

- Confirm in a live session that affected hair draws whole.
