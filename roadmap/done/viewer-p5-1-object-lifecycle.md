---
id: viewer-p5-1
title: Object lifecycle
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 5 — Prim rendering in the viewer
---

Context: [context/viewer.md](../context/viewer.md).

**P5.1. Object lifecycle.** New `objects.rs` module: an `ObjectState`
resource keying every in-world object by `ScopedObjectId`, folded from the
session event stream by the `update_objects` system. On
`ObjectAdded` / `ObjectUpdated` it spawns/updates an entity tagged with a
`SceneObject { scoped_id, category }` marker classifying it (avatar / plain
prim / sculpt / mesh / other, from `pcode` + the sculpt/mesh `ExtraParams`);
on `ObjectRemoved` it despawns the entity (Bevy's hierarchy takes its parented
children) and drops it plus any tracked descendants from the map. A **root**
object's `Transform` is a world transform (`sl_to_bevy_vec` position +
`sl_to_bevy_object_rotation` — the basis change composed with the object's own
orientation); a **child** gets a *local* transform kept in pure Second Life
space (`sl_rotation_to_quat`), parented via `ChildOf` to its root so the root
carries the single basis change for the whole linkset. A child that arrives
before its root is held parentless and adopted once the root appears
(`adopt_pending_children`); a runtime relink/unlink re-parents on update
(`reconcile_parent`). A `ShapeFingerprint` (pcode, the quantized
`PrimShapeParams`, and the sculpt/mesh key) is compared per update so a
motion-only update never flags a re-tessellation (consumed in P5.2). Two
rotation helpers were added to
`coords.rs` (`sl_rotation_to_quat`, `sl_to_bevy_object_rotation`). No geometry
is spawned yet — the entities carry only a `Transform` + marker, which P5.2 /
P7 / P9 / P10 hang meshes on. This stays a `sl-client-bevy-viewer`-only change
(no region-origin offset yet: objects sit in the root region's frame, aligned
with the home terrain and camera, as P2 does).
