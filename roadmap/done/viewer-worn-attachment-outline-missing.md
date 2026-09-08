---
id: viewer-worn-attachment-outline-missing
title: Selecting a worn rigged attachment highlighted nothing at all
topic: viewer
status: done
origin: found A/B-ing the mesh wireframe on aditi (2026-09-08)
refs: [viewer-mesh-objects-outlined-by-a-shell, viewer-edit-outline-skinned-mesh]
---

Context: [context/viewer.md](../context/viewer.md).

Selecting a worn **rigged** attachment in build mode left it with only the gizmo
handles — no outline of any kind, where an in-world animesh and a prim
attachment both wore one. Found while A/B-ing
[[viewer-mesh-objects-outlined-by-a-shell]] against Firestorm on aditi, and
older than that work: the highlight has never reached these faces.

The highlight reconcilers find a selected object's faces by walking the entity
hierarchy down from the object's own entity. A worn rigged submesh is not in
that subtree: `rigged_attachments::build_rigged_submeshes` parents it under the
**wearer avatar's body root**, because its vertices are placed by the joint
palette and the entity only carries lifecycle and visibility. So the walk found
nothing to outline, silently — an object with no faces looks exactly like an
object whose faces have not rezzed yet.

The way back is the one the GPU pick already uses: each such submesh carries
`WornPickTarget { scoped }` naming the worn object it renders, precisely because
"a hit on it cannot be walked up the entity hierarchy to a `SceneObject`". The
walk now also notes every object it crosses by scoped id (root, linkset child,
and their outline colours), and a single pass over the worn faces afterwards
claims the ones whose object is in that ledger. Both reconcilers — the selection
outline and the inventory drag-drop hover outline — get it, since both go
through the same walk.

A rigged face always wears the wireframe
([[viewer-edit-outline-skinned-mesh]]), so the object-kind and scale a shell
would need are not consulted for these.
