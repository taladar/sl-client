---
id: viewer-mesh-objects-outlined-by-a-shell
title: An unrigged mesh object is outlined by a hull where the reference wireframes it
topic: viewer
status: bugs
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

This viewer draws an inverted-hull shell for everything, except that
[[viewer-edit-outline-skinned-mesh]] gave **rigged** faces the wireframe. So an
unrigged uploaded mesh — most in-world content that is not a prim — still wears
the shell where the reference wireframes it.

The machinery is already there (`selection_wireframe::wireframe_mesh`, and the
`spawn_outline_overlay` split in `edit_selection.rs`); what is missing is the
predicate. A face entity does not currently know whether its object came from a
mesh asset, so this needs the object's shape kind carried to (or resolvable
from) the face, then the same wireframe path taken for it.

Worth deciding rather than assuming: the shell is a *better-looking* highlight
than a one-pixel wireframe (`wgpu` has no line-width state, so ours cannot be
the reference's five pixels), and on a dense mesh a full wireframe is a lot of
lines. A/B both against Firestorm on the same object before switching — the
shell may be the one place where the divergence is worth keeping, and if so this
becomes a `wont-do` with the reason written down.
