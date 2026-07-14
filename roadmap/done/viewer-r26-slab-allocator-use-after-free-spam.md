---
id: viewer-r26
title: bevy_render slab-allocator "use-after-free / unallocated key" log spam
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**FIXED.** Root cause was **zero-vertex meshes reaching the mesh allocator**:
`MeshAllocator::allocate_meshes` skips allocating a mesh whose vertex buffer is
zero bytes (`if vertex_buffer_size == 0 { continue }`) but its copy loop still
calls `copy_element_data` for it → the key is absent from `key_to_slab` → the
error, once per frame the mesh is modified. The per-frame producer was
`drive_particles`, which re-inserted every cloud's billboard mesh
**every frame regardless of particle count** (`build_cloud_mesh` returns an
empty mesh for a cloud with no live particles), so an idle / between-bursts
source spammed it. Fix: only rebuild + insert a cloud's mesh when it has
particles; otherwise leave its mesh untouched and hide the entity (clouds now
start `Hidden` until they have geometry, and `Visibility` is only rewritten on a
change). Suspects P32.2 / P33.1 in the original triage were wrong — flexi
filters empty faces and the probe sphere is non-empty. Live-verified: the flood
is gone.

**R26. `bevy_render::slab_allocator` use-after-free spam.** The viewer logs a
flood of

```text
ERROR bevy_render::slab_allocator: Use-after-free: attempted to copy element
data for an unallocated key
```

while running against a live grid. It is Bevy's mesh-GPU-allocator complaining
that a mesh handle is referenced for rendering after its slab allocation was
freed — i.e. a mesh asset is mutated / removed while still referenced, racing
extraction. It is **not** from the P31.12 look-at work (that only writes joint
`GlobalTransform`s and reads resources — no mesh allocation); it was reported
as **new since a run "a few commits ago"**, so a recent committed change is the
likely origin.

Prime suspects, both of which touch mesh assets every frame or on stream:

- **P32.2 simulate flexi prims** ([[viewer-p32-2]]) — rebuilds a flexi prim's
  mesh geometry each frame as it droops, the classic trigger for the allocator
  freeing a slab still in flight.
- **P33.1 default reflection probe** ([[viewer-p33-1]]) — adds GPU render
  resources.

To do: bisect (run `HEAD` without the P31.12 working-tree change, then walk
back the recent viewer commits) to confirm the origin, then stop mutating /
respawning the offending mesh in place — reuse the handle or rebuild only when
the geometry actually changes, rather than every frame. The spam is cosmetic
(no observed visual corruption yet) but drowns the log and likely wastes
re-uploads.
