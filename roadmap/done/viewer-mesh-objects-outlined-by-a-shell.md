---
id: viewer-mesh-objects-outlined-by-a-shell
title: An unrigged mesh object is outlined by a hull where the reference wireframes it
topic: viewer
status: done
origin: split out while porting the outline to skinned faces (2026-09-08)
refs: [viewer-edit-outline-skinned-mesh, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

The reference splits the edit-selection highlight by **object kind**, not by
whether the object is rigged: `LLSelectMgr::renderSilhouettes` sends every
object whose volume `isMesh()` — an uploaded mesh asset, rigged or not — to
`renderMeshSelection_f`, which draws its selected faces as a thick
**wireframe**; only prims and sculpts get `renderOneSilhouette`'s silhouette
edge geometry.

This viewer drew an inverted-hull shell for everything, except that
[[viewer-edit-outline-skinned-mesh]] gave **rigged** faces the wireframe. So an
unrigged uploaded mesh — most in-world content that is not a prim — still wore
the shell where the reference wireframes it.

## What the predicate turned out to be

`LLVOVolume::isMesh()` is the sculpt block's stitching type
(`sculpt_type & LL_SCULPT_TYPE_MASK == LL_SCULPT_TYPE_MESH`), which this viewer
already classifies once at object build: `SceneObject.category ==
ObjectCategory::Mesh`. A face entity does not carry it, but `collect_faces`
already walks from the selection root down through the linkset, so it carries
the nearest enclosing object's category (and the accumulated scale) down to each
face. `spawn_outline_overlay` then takes the wireframe for a mesh object's face
or a rigged one, and the shell for everything else — prims, sculpts, trees,
grass, which is exactly the set the reference leaves on the silhouette path.

## The lift had to become a world distance

The rigged path's lift (the port of the reference's polygon offset) was a
clamped fraction of the mesh's own bounding diagonal. That works for a rigged
face, whose geometry is in metres. An **unrigged** mesh object's geometry is in
the asset's normalized space, with the object's Second Life size on the geometry
holder above it — so the metre clamps were being applied to normalized units,
and a twenty-metre mesh object would have worn its outline a hand's width off
its own surface. `wireframe_mesh` now takes the scale between the mesh's space
and metres: the diagonal is measured as drawn, the clamp is in metres, and each
vertex moves `lift / |scale · normal|` locally so a non-uniform object scale
does not stretch the offset. A unit scale — every rigged caller — leaves the
computation exactly as it was.

## The decision the item asked for

The item asked to A/B the shell against the wireframe before switching, on the
grounds that the shell may read better (`wgpu` has no line-width state, so ours
is one pixel where the reference's is five) and that a dense mesh is a lot of
lines. **A/B'd on aditi: the wireframe stays.** It reads well at one pixel in
the three silhouette colours, in-world mesh objects wireframe while prims and
sculpts keep the shell, and a rubber-band drag across mesh content did not
hitch — if anything the few hitches seen were on large linksets wearing the
*shell*, which is the path this change shrinks. So no memoization of the derived
line list, and no brightening (`renderOneWireframe` doubles the colour to carry
its five-pixel line; ours does not need to).

The one thing the A/B could not cover is a very large mesh object, so the
world-distance lift above is argued and unit-tested rather than seen.

## Found by the A/B

Worn rigged attachments were outlined by *nothing at all* — older than this
change and invisible until the wireframe made the surrounding cases work. Fixed
alongside: [[viewer-worn-attachment-outline-missing]].
