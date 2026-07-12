---
id: viewer-p31-3
title: Physics-shape-aware colliders
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.3. Physics-shape-aware colliders.** Replace P31.2's placeholder
scale-sized cuboid with a collider that matches the object's real
`LLPhysicsShapeType` and geometry. Fetch it from the CAPS
`GetObjectPhysicsData` (`ObjectPhysicsProperties`, surfaced as
`Event::ObjectPhysicsProperties`): **none** → no collider (a physics prim with
no collision shape); **convex hull** → an avian convex hull from the prim /
mesh vertices; **prim** → the tessellated prim / mesh geometry (or its convex
decomposition). Uses avian's `collider-from-mesh` (already a default feature)
over the geometry the viewer already tessellates. Matters once P32 / P34 add
genuine dynamic bodies that collide against these kinematic movers — until
then the cuboid is inert. **Done:** the whole
wire/proto/runtime stack for the physics data already existed (`sl-wire`'s
`object_physics` — the `PhysicsShapeType` (none / prim / convex-hull) +
`ObjectPhysicsData` types and the `GetObjectPhysicsData` LLSD codecs — plus
`Command::RequestObjectPhysicsData`, both `Event::ObjectPhysicsData` (cap
reply, keyed by full `ObjectKey`) and `Event::ObjectPhysicsProperties`
(event-queue push, keyed by `ScopedObjectId`), and
`CAP_GET_OBJECT_PHYSICS_DATA`, all wired through both runtimes), so the only
net-new library change was re-exporting `PhysicsShapeType` +
`ObjectPhysicsData` from **both** `sl-client-bevy` and `sl-client-tokio` (a
latent parity gap — only the sim-features `PhysicsShapeTypes` plural was
exported). In the viewer, `physics.rs` gained a `full_key` on
`PhysicalObject`, an `ObjectPhysicsShapes` resource, and three systems.
`request_object_physics_data` fires one `RequestObjectPhysicsData` per object
the first time it is flagged physical (guarded by a `requested` set — the
reliable path, since a grid only *pushes* `ObjectPhysicsProperties` on a
physics-material change, not on stream-in). `ingest_object_physics_data` folds
both delivery paths into the resource by full key (translating the push's
`ScopedObjectId` via a new `ObjectState::full_key`). And
`refine_physical_colliders` builds the shape-aware collider once the data and
geometry are both available: **none** removes the collider, **convex hull** is
a `Collider::convex_hull` of the object's own vertices, **prim** (or an
unrecognised type) is a `Collider::trimesh` of that geometry. The geometry is
gathered from the object's own faces via a new `GeometryHolder` marker on the
per-object geometry holder (so linkset child prims — which also parent to the
object entity — are excluded), each vertex scaled by the object scale into the
object-root entity's local frame (where the collider lives; the entity
`Transform` carries the basis change, matching how the same faces render
through the holder scale — the points are **not** pre-basis-changed). Collider
ownership moved entirely to `refine_physical_colliders`:
`drive_physical_objects` (P31.2) now only seeds the initial placeholder
cuboid, and a new `RefinedCollider { shape, from_geometry, scale }` records
what was built so a collider is rebuilt only on a real change (new shape data,
a resize, or the geometry finally uploading — retried each frame until then).
These colliders are inert on the kinematic movers themselves, so verification
is log-based. Verified live on OpenSim with two throwaway
`<Flags>Physics</Flags>` box OARs whose `<PhysicsShapeType>` the OAR
serializer round-trips (`bin/slclient-physics.oar` shape 2 / convex-hull,
`bin/slclient-physics-prim.oar` shape 0 / prim): each 1 m cube was requested,
its shape received, and refined to the matching collider from its 24
vertices — `ConvexHull collider from 24 vertices` and `Prim collider from 24
vertices` — with a clean quit and no panics / avian / wgpu errors. Six new
unit tests (shape-needs-geometry, resize detection, index-offset triangle
merging,
and building a convex hull / a trimesh from a cube).
