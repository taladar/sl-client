---
id: viewer-rigged-hair-triangle-pattern
title: Rigged mesh hair shows a pattern of individual triangles
topic: viewer
status: bugs
origin: live Aditi session after the Bevy 0.20 upgrade (2026-11-22)
refs: [viewer-mesh-hair-not-rendering]
---

Context: [context/viewer.md](../context/viewer.md).

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

## The open lead

The hair's two legacy materials are the only ones of this avatar's faces the
capture does not have. The `RenderMaterials` capability answered for 9 of the 11
materials requested while the avatar was away, and not for these two. So every
replay so far has drawn the hair without its real alpha mode, and possibly
without normal and specular maps.

Firestorm tracks material requests per region, so the region may serve them only
while the wearer is present. That is not established.

## To find out

- Capture with the avatar in the region (`SL_VIEWER_DUMP_DIR`, Ctrl+Alt+D). The
  capture now fetches what is missing and logs `capture INCOMPLETE` with the ids
  of anything it could not get.
- If the materials arrive, replay with them and see whether the triangles
  remain.
- If they do not, the grid is not serving them to Firestorm either, and the
  cause is elsewhere in how we draw this mesh. Compare the index order next: the
  reference cache-optimizes a mesh's indices, which changes which triangle
  writes depth first now that rigged alpha does.
