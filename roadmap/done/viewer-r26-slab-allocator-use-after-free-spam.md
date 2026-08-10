---
id: viewer-r26
title: bevy_render slab-allocator "use-after-free / unallocated key" log spam
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

## Fixed (2026-08-11) — live-verified on aditi

Root cause re-confirmed against the Bevy 0.19 source: `MeshAllocator`
`allocate_meshes` skips a mesh whose vertex buffer is zero bytes
(`if vertex_buffer_size == 0 { continue }`) so no slab key is created, but the
following copy loop calls `copy_element_data` for **every** extracted mesh —
including that one — and the key lookup misses, logging the "unallocated key"
error. It is asset-driven (fires whether or not the entity is visible), once per
frame the empty mesh is added or modified.

Audited every per-frame / on-stream mesh producer in the viewer for a
zero-vertex output. Two genuinely unguarded producers found and fixed; the
documented suspects (flexi, particles, terrain, prim/mesh LOD re-tessellation)
were all already guarded (`face.is_empty()` / fixed-grid geometry / shared
static quad) and are **not** the source.

1. **Name-tag / hover-text billboards — the dominant, bursty flood.**
   `build_tag_meshes` built page 0 from `build_tag_mesh_data`, which returns a
   single **empty** page whenever the tag has no renderable glyphs — a
   just-spawned tag whose name has not resolved yet, or one whose glyph atlas is
   still streaming. Both the add path (`empty_tag_mesh()` + `meshes.add`) and
   the in-place update path (`meshes.get_mut` + `write_page_mesh`) then handed
   the allocator a zero-vertex mesh. Because a name tag carries a per-frame
   distance line and tags stream in as avatars enter draw distance, this churns
   through the whole run — matching the 30× / 298× bursts. Fix: guard on
   `!page0.positions.is_empty()`; the tag rebuilds with geometry on the next
   `TextLayoutInfo` change (name resolves, or the distance line ticks), so
   nothing is lost and it self-heals.

2. **Mesh / rigged-mesh object submeshes — matches "objects not rezzed".**
   `build_mesh_submeshes` / `build_rigged_submeshes` skipped only
   `submesh.no_geometry`, but the decoder sets `no_geometry: false` whenever the
   LLSD map merely lacks the `NoGeometry` marker — a malformed / LOD-stripped
   submesh with an absent `Position` blob decodes to **zero vertices** yet
   passes that guard, becoming a zero-vertex Bevy mesh that both spams the
   allocator and renders nothing (the object face silently fails to rez). Fix: a
   new `Submesh::has_geometry()` (`!no_geometry && !positions.is_empty()`)
   replaces the bare `no_geometry` guard at both call sites and in the shared
   `to_bevy_meshes`.

Client-side unit tests added (`sl-mesh` `has_geometry`, `sl-client-bevy`
`to_bevy_meshes` skip, viewer `empty_page_writes_a_zero_vertex_mesh`).
Live-verified on aditi (2026-08-11): a ~4-minute session in a populated region
with several other avatars (so multiple nameplates spawning / resolving / their
per-frame distance lines updating, the exact pre-fix flood scenario) logged
**zero** `slab_allocator` / "unallocated key" errors, down from the 30× / 298×
of the recurrence. Producer #2 (a malformed / LOD-stripped object submesh) is
asset-dependent and was not forced live, but its guard + unit test can only
remove errors, never add them.

## ⚠️ Recurred (2026-08-10) — and now with visible corruption

The flood is **back** on aditi: 30 occurrences in one session and **298** in
another (both this date), in bursts through the whole run, not just startup.
Unlike the original "cosmetic" flood, a resident reported **some objects did
not look properly rezzed** — consistent with a mesh whose GPU slab was freed
while still referenced (it simply does not render). So this is no longer
purely log spam.

- **Not** the avian ground-probe work (`viewer-perf-avatar-ground-probe`
  Stage 1): the flood appears with `SL_VIEWER_GROUND_PROBE_SPATIAL` **off**
  too (30×), and that change adds colliders, not mesh churn. The higher
  count with it on (298×) was a **different, denser region** (more cloud /
  particle activity — R26's known trigger), so it is confounded; a
  same-region on/off A/B is needed before blaming it.
- Same signature and system as the original (`allocate_and_free_meshes` →
  `slab_allocator` "unallocated key"), so a producer is again handing the
  allocator an empty / churned mesh. The original cloud producer was fixed;
  re-bisect the recent viewer commits for a **new** per-frame / on-stream
  empty-mesh producer (flexi, particles, terrain patch rebuilds, prim-LOD
  re-tessellation are the mesh-mutating suspects). Capture the offending
  entity by turning the allocator's log into a one-shot with a backtrace.

## Original fix (2026, since regressed)

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
