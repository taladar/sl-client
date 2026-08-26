---
id: viewer-ecs-idiom-audit
title: Audit the viewer for state modelled beside the ECS rather than in it
topic: viewer
status: done
origin: crate-split work (2026-08) — patterns the world-layer moves kept surfacing
points: 8
refs: [build-structural-encapsulation-audit, build-split-viewer-crate]
---

Context: [context/viewer.md](../context/viewer.md).

Much of the viewer's world state is held in big keyed resources, and the systems
iterate the map rather than querying entities. This is the audit for where that
is worth reversing — not a call to dissolve the stores, but to find the places
where the ECS would carry it better.

## The shapes, and what the sweep found

Two searches, workspace-wide.

**A map keyed by `Entity`.** The strongest form of the shape: the key *is* the
entity, so the value is unambiguously a fact about it. **Twenty sites.** Eight
are local variables inside a single system (`edit_selection`'s highlight
reconcile, `shadow_visibility`'s per-light gather, `render_test`'s name index)
and one more (`avatar_complexity`'s `SceneLookup::particles`) is a per-frame
snapshot — none of them is state. That left **eleven persistent stores**, of
which **four converted** and seven did not.

**A map keyed by a wire id whose value holds an `Entity`.** In the world crates:
`ObjectState::objects`, `AvatarState::by_scoped`, `WaterState::region_planes`,
`ParcelBorderState::entities`, `PendingBuilds::builds`. The first two are the
scene mirror the wire reconciles against and are the store this audit exists to
*not* dissolve. The last one is the task's headline instance and did convert.
The two region maps did not, and the reason is worth recording: they key by
`RegionHandle` and a region **is not an entity**. Three separate stores
(`WaterState`, `ParcelBorderState`, `TerrainTextures::materials`) each carry a
per-region entity or handle, and a canonical region entity would collapse all
three — but that is a design move, not an audit finding, so it is left as one.

### What separates the four that converted from the seven that did not

One line: **a store keyed by `Entity` whose whole job is to outlive the entity
is not a component in disguise.** `RemovedComponents` hands back an id and
nothing else, so a cleanup that needs the *data* — which pick slot to free
(`gpu_pick::PickRegistry::entity_tags`), which browser surface to close
(`browser_widget::BrowserViewIndex`, whose doc comment says exactly this), which
studio to return to the pool (`material_preview`'s `bound` / `applied`,
reclaimed from `RemovedComponents<MaterialPreview>`) — has to have kept it
somewhere the despawn does not reach.

The four that converted are the ones whose reader runs while the entity is still
alive. Where they *did* have something to clean up afterwards, it was itself an
entity — and there the ECS has three answers, one of which each conversion used:
Bevy's hierarchy, a `linked_spawn` relationship, and a back-pointer component.

The three remaining non-conversions are each their own reason:
`raycast_index::RaycastIndexColliders::records` is the authoritative input of a
BVH rebuild — an index, not per-entity facts; `gpu_avatars::stage`'s
`real_skins` is one field of a coherent staging structure whose siblings (pools,
dedup maps, generations) are inherently resource-level; and
`avatar_complexity`'s `hidden` already self-heals (its restore pass drops an
entry whose entity is gone) and its `has_work()` gate reads better off a
resource than off a query.

## The four conversions

**Deferred build state as a component.** `PendingBuilds` was a
`HashMap<ScopedObjectId, ObjectBuilds>` whose own doc named the hazard the crate
split had introduced: *"an object's builds used to die implicitly with its
`TrackedObject`, and now every removal path has to say so. A missed one leaks a
queue entry for an object that no longer exists, which nothing else would
notice."* Four `ObjectState::remove_object` sites honoured it; nothing made
them.

`ObjectBuilds` is now a **component** on the object entity — one component, not
four, so a re-tessellation still states the object's whole outstanding work in a
single insert and cannot leave a stale rebuild input behind by forgetting one of
four. `PendingBuilds` survives as the name of a `SystemParam` over
`(Entity, &SceneObject, &mut ObjectBuilds)`, so the five call sites read almost
as they did. `forget`, `forget_all` and `clear` are gone, with their six
callers; so are the resource registration, the `scene_reset` clear, and
`apply_object`'s `builds` parameter (it now writes through `Commands`, which it
already had).

Two details that decided the shape:

- The **writes go through `Commands`, not the parameter.** A record is
  established for an entity `apply_object` may only just have spawned, so it
  must be a command; and giving the parameter its own second command queue would
  make the order of an insert against the outer queue's despawn undefined.
- `take_pending` deliberately leaves an emptied record rather than removing it,
  because `apply_object_meshes` re-parks a *different* build on the same entity
  in the same pass and a queued removal would land after that write. The two
  callers that know nothing more is coming (`apply_object_sculpts`,
  `apply_rigged_attachments`) prune explicitly via `drop_if_resolved`.

The honest accounting: this did **not** buy better indexing (an archetype scan
where there was a map scan is a wash — and the map was near-object-sized anyway,
because every plain prim retains its re-tessellation inputs for good) and it
removed **no** dependency (the store already lived in the same module as
everything that touched it). It bought exactly one thing: the leak is
structurally impossible again. `every_removal_path_forgets_the_deferred_builds`
became `every_removal_path_drops_the_deferred_builds`, and now pins that the
three ways an object leaves the scene need say nothing at all.

**A relationship where there was a `HashMap<Entity, Entity>`.**
`HoverTextLabels` mapped an object entity to the floating-text billboard it
owns. The billboard is deliberately *not* a child (the object subtree's
`Propagate(probe layers)` would leak a reflection-probe layer onto it), so the
hierarchy could not reap it and the map had to. `HoverText` is now a Bevy
**`Relationship`** with `HoverTextLabel` as its `linked_spawn` target — the
first relationship in this workspace. The object despawning now takes its
billboard with it, and `despawn_removed_hover_text` shrank to the one case that
is really about a live object: `llSetText("")`.

**Two reconcile-against-a-map passes became reconcile-against-a-query.**
`LocalLights::assigned` (`Entity → (Entity, ObjectLight)`) is now
`AssignedLight` on the light-flagged prim, and `ParticleSim::clouds`
(`Entity → Cloud`) is now `Cloud` on the particle source. Both had a `retain`
whose only job was to notice entries whose object had despawned; both lost it.
The particle driver also lost its `sources_data` snapshot `Vec` and `current`
`HashSet` — they existed solely to release the query borrow before mutating the
resource, which a `&mut Cloud` does not need. The cloud's *render* entity is
world-space (its particles are in absolute coordinates, so it must not inherit
the emitter's transform), so it keeps a `CloudOf` back-pointer and one small
`retire_orphaned_clouds` system — the audit's third answer to "the thing to
clean up is itself an entity".

**An in-flight async task as a component.** `StaticColliderBuilds::tasks`
(`Entity → StaticBuildTask`) is now the task component itself, which is Bevy's
own idiom for an `AsyncComputeTaskPool` job. `apply_static_colliders` went from
poll-into-a-`Vec`, re-`remove`, `commands.get_entity` liveness check, to a plain
`Query<(Entity, &mut StaticBuildTask)>` — a prim that despawned mid-build simply
is not in the query. `build_static_colliders` now reads "is a build already
running for this prim?" out of the archetype with the rest of the prim's
components, as `Option<&StaticBuildTask>` rather than a per-candidate hash
lookup. It stays fetched data rather than a `Without` filter on purpose: a
*disqualified* prim with a build in flight still has to be seen, to strip its
stale collider and cancel the build.

## Asset managers: the diagnostic half of the inversion

The task's third instance — *"`TextureManager`, `MeshManager`, … are **called**
by consumers that want a decoded result; publishing results would invert the
dependency"* — was measured before it was touched. `sl-viewer-world-scene` reads
into **twelve** modules of `sl-viewer-world-objects`. Five are asset managers;
seven are not (the shared `MeshUploadBudget`, the face-material machinery in
`bump` / `legacy_materials` / `material_cache`, `tag_render_layers`, the object
components `SceneObject` / `PrimFaceEntity` / `FaceTextureDebug`, and
`texture_anim`).

Then the surprise: **four of the five managers were named from exactly one
place** — `update_pipeline_overlay`, the `F3` debug panel, which reads nothing
from them but `stats()`, `gate_stats()` and `deferred_count()`. That is not a
call-target dependency at all; it is a diagnostic reading a uniform statistics
interface across nine stores.

So that half is inverted. `PipelineStats` in `sl-viewer-world-api` is a keyed
resource — `label → StorePipelineStats` — that the layer owning each store
publishes into, and `AssetStatsPlugin` in the object crate publishes its five.
The overlay reads one resource. It is **demand-driven**: the reader calls
`set_wanted`, the publisher's run condition reads it, so a hidden overlay costs
one boolean check rather than five stats snapshots. The label constants live in
`world-api` beside the resource, because they are what the two layers agree on
rather than either one's private naming.

Result: `sl-viewer-world-scene` no longer names `AnimationManager`,
`MaterialManager`, `MeshManager` or `WearableAssetManager` at all — **twelve
module dependencies down to eight**. The crate dependency stays, and would even
if `TextureManager` were inverted too: the scene layer genuinely needs the face
material machinery and the object components. That is the answer to the third
question for this instance — *it removes four of twelve, not the dependency.*

## Scheduling by system name: measured, not moved

The task recorded fifty-two world systems named in the composition root's own
ordering constraints. Re-measured against the current tree — every `pub` /
`pub(crate) fn` in the four world crates, matched against the root's `.after(…)`
/ `.before(…)` arguments — that is **nine**: `recenter_terrain`,
`recenter_objects`, `recenter_avatars`, `update_objects`, `apply_object_meshes`,
`update_avatar_objects`, `update_coarse_avatars`, `apply_avatar_names`,
`drive_render_priority`.

[build-structural-encapsulation-audit](build-structural-encapsulation-audit.md)
took the other forty-three, and the nine that are left are the ones it named as
hard: the terrain-wins-the-shared-`MeshUploadBudget` chain and the avatar chain
both order *both ways* across the layers. Dissolving them is
[build-split-world-avatar-crate](../done/build-split-world-avatar-crate.md)'s
work, which has to move that chain anyway. Recording the number so the next
person does not re-derive it.

## What this cost and what it is worth

**+934 / −489 lines across thirteen files**, four resources deleted
(`PendingBuilds`, `LocalLights`, `ParticleSim`, `StaticColliderBuilds`) plus
`HoverTextLabels`, one added (`PipelineStats`, in the layer below both its
writer and its reader). Six new tests pin the conversions, each on the property
the map could not state: that a despawned source's cloud render entity is
reaped, that a budgeted prim keeps the *same* light child across an idle frame,
that `linked_spawn` takes a billboard with its object, that a finished collider
build installs and clears itself, that the three removal paths drop an object's
deferred builds without saying so, and that the asset stores publish nothing
while nothing is looking.

## Live verification

Checked against the local OpenSim grid: prims / meshes / sculpts rez and
re-tessellate, floating text shows and clears, prim lights hold steady without
the per-frame flicker the keep-alive reconcile exists to prevent, and the `F3`
panel's five published store lines carry live figures.

**The particle conversion is committed but not live-verified** — the local grid
carries no particle emitters, so `drive_particles` / `retire_orphaned_clouds`
were exercised only by their unit test. What to watch for when a grid with
emitters is next to hand: a stream that flickers on and off every frame would
mean the sync point between the driver and the reaper is not landing where this
assumes it does.

The `F3` panel also made an existing bug newly visible rather than causing it —
the stuck asset-retry counter, [viewer-asset-retry-counter-stuck][retry]. The
published `def` column is simply the first place the permanently-deferred
texture was easy to see.

[retry]: viewer-asset-retry-counter-stuck.md

The general lesson, for the next time this shape turns up: **the question is not
whether the data is per-entity — it is whether anything reads it after the
entity is gone.** Four stores that did not, moved. Three that did, stayed, and
were right to.
