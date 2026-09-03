---
id: build-structural-encapsulation-audit
title: Audit the workspace for structural and encapsulation improvements the crate split exposed
topic: viewer
status: done
origin: crate-split work (2026-08) — patterns the visibility pass kept surfacing
points: 8
refs: [build-split-viewer-crate]
blocked_by: [build-split-viewer-crate]
---

Splitting the viewer turned a style question into a mechanical one: the
compiler names every item used across a new boundary, so each crate's API
arrives as a list rather than a judgement. That list kept pointing at the same
handful of structural problems, and most were worked around rather than fixed
because the split commits were already large. This task is the pass that fixed
them, once the split was done and the boundaries had stopped moving.

## What it measured

The four world crates carried **820 `pub` items**. The question the task set
was how many of those are `pub` for no reason of their own — reachable only
because they appear in the signature of a system the viewer schedules.

Answering it mechanically: demote every `pub` item that is never *named*
outside its crate, then read the compiler. `private_interfaces` is `forbid`
workspace-wide, so each failure names the exact public item that holds the
private one up.

**55 items** were never named outside their crate. The errors split them two
ways, and only one of the two is a prize:

| Held up by | Count | Verdict |
| --- | --- | --- |
| a `pub` **system** the viewer schedules | 46 | the prize |
| a `pub` **accessor / field** that returns it | 9 | not a prize |

The second group is the one the "never named outside" heuristic gets wrong:
`AvatarState::map_avatars()` is called from the radar and the minimap, but
callers iterate the result and read fields, so the type `MapAvatar` is never
spelled. Those types are genuinely part of the API. The same holds for
`RegionTerrain`, `ObjectEditData`, `StaticColliderFacts`, `AvatarComplexity`,
`JellyReason`, `ReplayManifest`, `GpuPickHit` and `ActiveMedia`.

So the honest prize was **46 of 820 items (5.6%)**, held up by **39 systems**
whose registration lived in the viewer's `lib.rs`.

### What the rework actually cost

Less than the count suggests, because the pattern was already established:
26 of the world crates' 76 modules shipped a `Plugin` before this task, and
`WorldPhase` already existed as the vocabulary for ordering across a boundary
("naming a system across a boundary is a dependency on the code that produces
the world, not on the world it produced").

The blocker was never "no plugin". `hover_text` and `name_tag_billboard` both
had plugins whose doc comment said *"the systems are scheduled alongside the
avatar systems in `lib.rs`"* — because their ordering edges named concrete
systems (`.after(update_objects)`, `.after(position_camera)`) and one of the
two ends was always in another crate. Growing `WorldPhase` by five variants
dissolved that:

- `CameraPositioned` — 15 edges, all from the scene layer *below* the camera
- `AvatarControlsDriven` — the object layer's locomotion animations read the
  intent the view layer just advertised
- `AvatarSkeletonsDriven` — the name-tag composer waits on it from a crate
  *above*, because it also waits on the group store
- `AvatarAppearanceApplied` / `AvatarMorphsFolded` — the appearance rebuild and
  the morph fold, which the animation pipeline brackets

One more item moved rather than gained a set: `world_has_keyboard` was a run
condition living in `world_view::input_context` while `InputContext` itself
already lived in `world_api`. A gate stated in shared vocabulary can be applied
by any layer, and three layers need it. Moving the function down beside the
enum is what let the animation pipeline and the crosshair pick tool schedule
themselves.

### Result

**820 → 688 `pub` items in the world crates (−132, −16%).** More than the 46
because a system that stops being scheduled from outside stops being `pub`
itself. `sl-viewer-world-scene` is at **zero** items that no other crate names.

Fourteen new plugins own their own scheduling: `SkyPlugin`, `WaterPlugin`,
`WaterExclusionPlugin`, `ParticlesPlugin`, `LocalLightsPlugin`,
`PipelineOverlayPlugin`, `AvatarMovementPlugin`, `HudScreenPlugin`,
`ScreenshotPlugin`, `AvatarAppearancePlugin`, `AvatarAnimationPlugin`,
`AvatarPosePlugin`, `AnimeshPosePlugin`, `ObjectDiagnosticsPlugin` — plus the
render chain moving into `NameTagBillboardPlugin` / `HoverTextPlugin` and the
fog driver into `UnderwaterFogPlugin`. Six env-gated debug switches
(`SL_VIEWER_CAMERA_DUMP`, `SL_VIEWER_PARTICLE_FOCUS`, `SL_VIEWER_VOLUME_FOCUS`,
`SL_VIEWER_LOG_OBJECTS`, `SL_VIEWER_LOG_AVATAR_INTEREST`,
`SL_VIEWER_PIPELINE_OVERLAY`) moved with them: which system a diagnostic must
follow is the owning module's business, not the viewer's.

`lib.rs` lost roughly 300 lines of registration and, with it, the property that
it had to know every system in the world layer.

## Render handles held where the state lives

The placeholder avatar is one shared UV-sphere mesh and one shared soft-blue
material. `AvatarPlaceholderAssets` had already been lifted out of
`AvatarState` during the split — enough to move the store below the world — but
the spawn path still threaded it, plus `Assets<Mesh>` and
`Assets<FaceMaterial>`, through four helpers into three systems.

Now `spawn_sphere` spawns a *marked position*: `AvatarSphere`, a transform and
a `Visibility`, and nothing else. `dress_avatar_spheres` attaches `Mesh3d` /
`MeshMaterial3d` to any `AvatarSphere` that lacks them, ordered after
`WorldPhase::AvatarsUpdated` so Bevy's auto-inserted sync point flushes the
spawn first and a sphere is dressed in the frame it appears.

That removed the placeholder resource and both asset stores from
`update_avatar_objects`, `update_coarse_avatars` and `enforce_derender`, and
from `apply_object`, `apply_coarse`, `derender_agent` and `spawn_sphere` — with
three `too_many_arguments` expectations along with them. Covered by
`avatars::tests::spheres_are_dressed_from_the_marker_and_share_one_mesh`.

## Vocabulary separated from the state defined in terms of it

`MatModeState`'s three fields were `usize` indices whose meaning lived in nine
`MATMEDIA_*` / `MATTYPE_*` / `PBRTYPE_*` constants. The split had already
co-located them, but state described by loose constants still cannot describe
itself.

They are now `MatMedia`, `MatChannel` and the pre-existing `PbrChannel`, with
`MAT_MEDIA_MODES` / `MATERIAL_CHANNELS` / `PBR_CHANNELS` as the one place the
index↔value mapping lives and `radio_index()` / `from_radio_index()` as the
conversion at the widget edge — exactly the shape `BUILD_TOOLS` and
`EditTool::radio_index` already used for the tool radio. The nine constants and
the `pbr_channel()` resolver are gone; `is_material()` / `is_pbr()` are now
`matches!` over an enum rather than an index comparison.

## The recurring patterns: what the workspace-wide grep found

Three of the five patterns were searched for across the workspace and came back
essentially clean. That is a result, not a gap — recording it so the next
person does not re-run the search.

- **Read-then-mark that should be one step.** Every `pub` `bool` field that is
  both guarded on and assigned, workspace-wide. The wire-edge latches this was
  aimed at are already claim-and-test (`MuteModel::claim_request`,
  `PresenceState::take_away_edge` / `take_dnd_edge`,
  `InventoryModel::claim_cof_prefetch`), and the set-based ones already use
  `HashSet::insert`'s return (`physics`, `hover_tooltip`, `ui_texture_picker`).
  What is left is `dirty = false` after a save — a different, benign shape,
  because a missed clear costs one redundant write, not a divergent advertised
  state. The one true latch outside that set,
  `NearbyRecallState::requested`, is a private single-file resource with one
  caller, where a second requester cannot appear without editing the file that
  owns it.
- **A data model carrying presentation state.** Every `*Model` struct scanned
  for sort / scroll / expansion / selection fields. One hit:
  `InventoryModel::expanded`. Unlike `FriendsModel`'s column sort, it is not
  blocking anything — the model already lives in the feature crate that draws
  it — so lifting it would be churn without a payoff.
- **A predicate stated in the wrong tier.** The instance this generalised from
  (`shows_autoresponse`) was fixed during the split. The one further instance
  found is the `world_has_keyboard` move described above.

## What is left, and why

`WorldRootObject` is the one remaining component held up by an exported system:
`recenter_objects`. Freeing it means moving the scene-reset / recenter chain,
which orders *both* ways across the layers — `scene_reset` (view) runs before
the three recenter systems, and the terrain chain (scene) runs before
`update_objects` and `apply_object_meshes` (objects) to win the shared
`MeshUploadBudget`. That is three more `WorldPhase` variants for one component,
so it was left alone; it is the natural first step of
[[build-split-world-avatar-crate]], which will have to move that chain anyway.

The other nine remaining items are the accessor-held group above. They are
`pub` because they are genuinely reachable, and demoting them would be wrong.

## Not done, deliberately

`module_name_repetitions` is still expected crate-wide in five crates. Renaming
to satisfy it would churn every call site for a style rule this codebase does
not follow.
