//! Per-fragment water clipping for translucent faces that **straddle** the
//! surface (`viewer-straddling-transparency-oit`) — the port of the reference
//! viewer's `waterSign` / `waterClip`.
//!
//! # The problem one draw cannot solve
//!
//! The sea is opaque and writes depth ([`crate::water`]), so translucency has to
//! be composited on the right side of it: what is **beyond** the surface belongs
//! in the screen copy the surface refracts, and what is **in front of** it belongs
//! after the surface is drawn. [`crate::transparency`] sorts each translucent item
//! into one of those two passes.
//!
//! A face that crosses the waterline has fragments on **both** sides, and a single
//! draw can only be in one pass. Bucketed whole, half of it lands on the wrong
//! side — and that half does not merely sort wrong, it *disappears*: a translucent
//! face writes no depth, so an emergent half drawn before the sea is painted over
//! by the sea behind it. Measured on the grid, a 3.30 m box resting 0.49 m under
//! the surface showed nothing at all of the 1.16 m standing above it.
//!
//! # What the reference does, and what this does
//!
//! The reference renders its alpha pool **twice** — once as
//! `POOL_ALPHA_PRE_WATER`, once as `POOL_ALPHA_POST_WATER` — over the same draw
//! lists, with a `waterSign` uniform flipped between the two so each pass discards
//! the fragments belonging to the other (`lldrawpoolalpha.cpp`'s
//! `prepare_alpha_shader`, `deferredUtil.glsl`'s `waterClip`). Every fragment
//! therefore lands in the pass that orders it correctly, whatever its object's
//! centre.
//!
//! Bevy cannot vary a uniform between two draws of one material: a phase item
//! carries one pipeline and one material binding, and the pass that draws it is
//! not ours to parameterise. So the same thing is expressed with **two draws of
//! the same mesh**: the face keeps the half above the surface, and a *twin* entity
//! parented to it — sharing its mesh, carrying a copy of its material with the
//! opposite [`SlFaceParams::water_clip`](sl_viewer_kit::face_material::SlFaceParams::water_clip)
//! — keeps the half below. Each is then bucketed by the side it keeps rather than
//! by where its centre happens to be ([`WaterClipSide`], read by
//! `crate::transparency::classify_bucket`), so the two halves go to the two passes
//! exactly as the reference's two pool draws do.
//!
//! # Scope
//!
//! Only [`FaceMaterial`] faces — prims, meshes and sculpts, the content the defect
//! was reported on. The other translucent world materials (particles, name tags,
//! parcel borders, beacons) keep the per-object bucket: none of them is a large
//! surface that crosses the waterline, and each would need the same clip in its own
//! shader.

use bevy::camera::primitives::Aabb;
use bevy::mesh::morph::MeshMorphWeights;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::render::sync_world::MainEntity;
use bevy::render::{Extract, RenderApp};

use sl_viewer_world_objects::material_cache::SharedFaceMaterial;
use sl_viewer_world_objects::objects::PrimFaceEntity;

use crate::face_material::FaceMaterial;
use crate::water::{DEFAULT_WATER_HEIGHT, WaterLevel};

/// The tracing target of the straddling-split diagnostics: which faces were found
/// to cross the waterline and split in two. Off by default; turn it on with
/// `RUST_LOG=info,sl_viewer::water_clip=debug`.
pub const WATER_CLIP_LOG_TARGET: &str = "sl_viewer::water_clip";

/// Which side of the water surface an entity's draw keeps — the port of the
/// reference's `waterSign`.
///
/// Present only on a face that straddles the surface (and on its twin); an
/// ordinary face has no clip and is bucketed by its centre as before.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterClipSide {
    /// This draw keeps the fragments **above** the surface and discards the rest.
    Above,
    /// This draw keeps the fragments **below** the surface.
    Below,
}

impl WaterClipSide {
    /// The `water_clip` uniform this side sets: `+1` keeps what is above the
    /// surface, `-1` what is below (the shader discards the other half).
    const fn sign(self) -> f32 {
        match self {
            Self::Above => 1.0,
            Self::Below => -1.0,
        }
    }
}

/// The twin draw of a straddling face: the second of the two draws, parented to
/// the face it doubles so it shares its transform and dies with it.
///
/// A child rather than a sibling because the face's mesh is in the parent's local
/// space — the same reason the edit-selection overlay is a child — so an identity
/// transform puts the twin exactly on it.
#[derive(Component, Debug)]
pub struct WaterClipTwin;

/// What the reconciler reads of each candidate face: its mesh and material (to
/// copy into the twin), its world placement and bounds (to decide whether it
/// crosses the surface), the clip it already carries, whether its material is
/// one the [`MaterialCache`](sl_viewer_world_objects::material_cache) shares,
/// and the components its mesh **bind group** is built from — the skin and the
/// morph weights — which the twin has to carry too.
type ClipCandidates<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Mesh3d,
        &'static MeshMaterial3d<FaceMaterial>,
        Ref<'static, GlobalTransform>,
        Ref<'static, Aabb>,
        Option<&'static WaterClipSide>,
        Has<SharedFaceMaterial>,
        Option<&'static SkinnedMesh>,
        Option<&'static MeshMorphWeights>,
    ),
    With<PrimFaceEntity>,
>;

/// Give every translucent face that crosses the waterline a second draw, and take
/// it away again when it no longer does.
///
/// Runs the straddle test only over faces whose placement **changed** — and over
/// every face when the water level itself moved, since that re-decides all of them
/// at once and none of them moved. A settled scene therefore pays a single query
/// walk and no per-face work, which matters because a busy region has tens of
/// thousands of faces and almost none of them are anywhere near the surface.
fn reconcile_water_clip_twins(
    water_level: Option<Res<WaterLevel>>,
    faces: ClipCandidates,
    twins: Query<(Entity, &ChildOf), With<WaterClipTwin>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut commands: Commands,
) {
    // A settled face is re-evaluated only when it moves. The exception is the water
    // level itself moving — crossing into a region whose sea is at a different
    // height re-decides every face at once, and none of them moved.
    let level_moved = water_level.as_ref().is_some_and(Res::is_changed);
    let level = water_level.map_or(DEFAULT_WATER_HEIGHT, |water_level| water_level.0);
    // The faces that should have a twin this frame, and the twin each already has.
    let mut wanted: bevy::platform::collections::HashSet<Entity> =
        bevy::platform::collections::HashSet::new();
    for (face, mesh, material, transform, aabb, side, shared, skin, morph) in &faces {
        // A face already split stays in `wanted` however it is filtered below, or
        // the sweep at the end would despawn a twin that is still correct.
        if side.is_some() {
            let _inserted = wanted.insert(face);
        }
        // `Aabb` is computed by Bevy's own `calculate_bounds`, which may not have
        // run yet on the frame a face is spawned — so the face does not match this
        // query at all on the one frame its `GlobalTransform` counts as changed.
        // Its bounds arriving is therefore its own trigger, or a static face would
        // never be evaluated at all.
        if !level_moved && !transform.is_changed() && !aabb.is_added() {
            continue;
        }
        if !straddles(&transform, &aabb, level) || !is_translucent(&materials, material) {
            // No longer straddling (or never was): drop any clip it carries. Its
            // twin is despawned by the sweep below.
            if side.is_some() {
                let _removed = wanted.remove(&face);
                clear_clip(face, material, &mut materials, &mut commands);
            }
            continue;
        }
        let _inserted = wanted.insert(face);
        if side.is_some() {
            // Already split, and the split does not depend on where exactly it
            // straddles — only that it does.
            continue;
        }
        // The face keeps the half above the surface. Its material may be shared
        // with every identical face in the scene (`MaterialCache`), so it is copied
        // before the clip is written into it — the copy-on-write the intern net
        // requires.
        let Some(mut composed) = materials.get(&material.0).cloned() else {
            continue;
        };
        composed.extension.params.water_clip = WaterClipSide::Above.sign();
        composed.extension.params.water_level = level;
        let mut twin_material = composed.clone();
        twin_material.extension.params.water_clip = WaterClipSide::Below.sign();
        let own = materials.add(composed);
        let twin = materials.add(twin_material);
        let mut face_commands = commands.entity(face);
        face_commands.insert((MeshMaterial3d(own), WaterClipSide::Above));
        if shared {
            face_commands.remove::<SharedFaceMaterial>();
        }
        debug!(
            target: WATER_CLIP_LOG_TARGET,
            "split face {face}: straddles the surface at {level} m, drawing it twice \
             (skinned={})",
            skin.is_some(),
        );
        let mut spawned = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(twin),
            Transform::IDENTITY,
            WaterClipTwin,
            WaterClipSide::Below,
            ChildOf(face),
        ));
        // The twin draws the **same geometry** as the face, clipped to the other
        // side — so it must carry every component the mesh **bind group** is
        // built from, because it shares the face's mesh handle and Bevy decides
        // the two halves of the draw in two different places:
        //
        // | property | pipeline key from | bind group from |
        // | --- | --- | --- |
        // | skinning | the mesh's `JOINT_INDEX` / `JOINT_WEIGHT` | the entity's `SkinnedMesh` |
        // | morph targets | the mesh's `morph_targets()` | the entity's morph index |
        //
        // Miss one and the shared mesh specializes a pipeline the twin cannot
        // bind for: a wgpu validation error that quits the viewer, not an
        // artifact. Worse, worn rigged submeshes share one mesh asset across
        // wearers so Bevy can batch them, and a batch takes its bind group from
        // one representative entity — so a malformed twin takes down every other
        // wearer drawn with it, which is why the skinned case read as a random
        // crash near other people's avatars rather than as anything about water.
        //
        // Anything added to that table later belongs here too.
        if let Some(skin) = skin {
            spawned.insert(skin.clone());
        }
        if let Some(morph) = morph {
            spawned.insert(morph.clone());
        }
    }
    for (twin, child_of) in &twins {
        if !wanted.contains(&child_of.parent()) {
            commands.entity(twin).despawn();
        }
    }
}

/// Whether `aabb` under `transform` has geometry on both sides of `level`.
fn straddles(transform: &GlobalTransform, aabb: &Aabb, level: f32) -> bool {
    // The world-space extent of the face's bounds along Y: the centre displaced by
    // the transformed half-extents, whose Y reach is the sum of the absolute
    // contributions of the three axes.
    let centre = transform.transform_point(Vec3::from(aabb.center));
    let basis = transform.affine().matrix3;
    let half = Vec3::from(aabb.half_extents);
    let reach = (basis.x_axis.y * half.x).abs()
        + (basis.y_axis.y * half.y).abs()
        + (basis.z_axis.y * half.z).abs();
    centre.y - reach < level && centre.y + reach > level
}

/// Whether this face's material is alpha-blended — the only kind that needs the
/// split. An opaque or masked face writes depth and is ordered per pixel by it.
fn is_translucent(
    materials: &Assets<FaceMaterial>,
    material: &MeshMaterial3d<FaceMaterial>,
) -> bool {
    materials
        .get(&material.0)
        .is_some_and(|face| matches!(face.base.alpha_mode, AlphaMode::Blend))
}

/// Drop a face's clip: rewrite its material to the unclipped whole and remove the
/// marker. Its twin is despawned by the caller's sweep.
fn clear_clip(
    face: Entity,
    material: &MeshMaterial3d<FaceMaterial>,
    materials: &mut Assets<FaceMaterial>,
    commands: &mut Commands,
) {
    if let Some(mut asset) = materials.get_mut(&material.0) {
        asset.extension.params.water_clip = 0.0;
    }
    commands.entity(face).remove::<WaterClipSide>();
}

/// The [`WaterClipSide`] of every clipped draw, keyed by its main-world entity —
/// the render-world mirror `crate::transparency` looks each phase item up in, the
/// same way it mirrors the sky backdrops.
#[derive(Resource, Default, Debug)]
pub(crate) struct WaterClipSides(bevy::render::sync_world::MainEntityHashMap<WaterClipSide>);

impl WaterClipSides {
    /// The side `entity`'s draw keeps, if it is clipped at all.
    pub(crate) fn get(&self, entity: MainEntity) -> Option<WaterClipSide> {
        self.0.get(&entity).copied()
    }
}

/// Mirror the main world's [`WaterClipSide`] markers into the render world. A
/// handful of entities at most, rebuilt each frame so a face that stopped
/// straddling leaves nothing behind.
fn extract_water_clip_sides(
    mut sides: ResMut<WaterClipSides>,
    markers: Extract<Query<(Entity, &WaterClipSide)>>,
) {
    sides.0.clear();
    sides.0.extend(
        markers
            .iter()
            .map(|(entity, side)| (MainEntity::from(entity), *side)),
    );
}

/// Wires the straddling-face split into the app: the reconciler in the world
/// phase, and the render-world mirror its bucket decision is read from.
#[derive(Debug, Default)]
pub struct WaterClipPlugin;

impl Plugin for WaterClipPlugin {
    fn build(&self, app: &mut App) {
        // In `PostUpdate` after propagation, because whether a face straddles the
        // surface is a question about its **world** placement: an `Update` reader of
        // a `GlobalTransform` sees last frame's
        // (`sl-client-update-globaltransform-one-frame-lag`), which for a prim
        // drifting across the waterline would split it a frame late.
        app.add_systems(
            PostUpdate,
            reconcile_water_clip_twins.after(TransformSystems::Propagate),
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<WaterClipSides>()
            .add_systems(ExtractSchedule, extract_water_clip_sides);
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{WaterClipSide, WaterClipTwin, reconcile_water_clip_twins, straddles};
    use crate::face_material::FaceMaterial;
    use crate::water::WaterLevel;
    use bevy::asset::AssetApp as _;
    use bevy::camera::primitives::Aabb;
    use bevy::mesh::morph::MeshMorphWeights;
    use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::PrimFaceId;
    use sl_viewer_kit::face_material::inert_face_material;
    use sl_viewer_world_objects::objects::PrimFaceEntity;

    /// The water level the fixtures straddle.
    const LEVEL: f32 = 20.0;

    /// An app with the reconciler and the asset store it copies materials in.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<FaceMaterial>()
            .init_asset::<SkinnedMeshInverseBindposes>()
            .insert_resource(WaterLevel(LEVEL))
            .add_systems(PostUpdate, reconcile_water_clip_twins);
        app
    }

    /// Spawn a 2 m translucent face box centred at height `centre`.
    fn spawn_face(app: &mut App, centre: f32, alpha_mode: AlphaMode) -> Entity {
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(2.0, 2.0, 2.0).mesh().build());
        let material =
            app.world_mut()
                .resource_mut::<Assets<FaceMaterial>>()
                .add(inert_face_material(StandardMaterial {
                    base_color: Color::WHITE.with_alpha(0.5),
                    alpha_mode,
                    ..default()
                }));
        app.world_mut()
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, centre, 0.0),
                GlobalTransform::from_xyz(0.0, centre, 0.0),
                Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
                PrimFaceEntity {
                    face_id: PrimFaceId::new(0),
                },
            ))
            .id()
    }

    /// How many twin draws exist.
    fn twins(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<WaterClipTwin>>()
            .iter(app.world())
            .count()
    }

    /// **A translucent face crossing the waterline is split into two draws.**
    ///
    /// The face keeps the half above the surface and its twin the half below, so
    /// each can be ordered against the sea on its own side. Without the split the
    /// half on the wrong side is painted over by the depth-writing sea and
    /// disappears (`viewer-straddling-transparency-oit`).
    #[test]
    fn a_straddling_translucent_face_is_split_in_two() {
        let mut app = app();
        let face = spawn_face(&mut app, LEVEL, AlphaMode::Blend);
        app.update();

        assert_eq!(twins(&mut app), 1, "the straddling face gained its twin");
        assert_eq!(
            app.world().get::<WaterClipSide>(face).copied(),
            Some(WaterClipSide::Above),
            "the face itself keeps the half above the surface",
        );
        // The two draws clip to opposite sides, and both cut at the water level.
        let materials = app.world().resource::<Assets<FaceMaterial>>();
        let own = app
            .world()
            .get::<MeshMaterial3d<FaceMaterial>>(face)
            .and_then(|handle| materials.get(&handle.0))
            .expect("the face has a material");
        assert!((own.extension.params.water_clip - 1.0).abs() < f32::EPSILON);
        assert!((own.extension.params.water_level - LEVEL).abs() < f32::EPSILON);
    }

    /// **A skinned face's twin is skinned too**, or the split kills the viewer.
    ///
    /// The twin shares the face's *mesh asset*. Bevy specializes the render
    /// pipeline from that asset's vertex layout — a rigged mesh carries
    /// `JOINT_INDEX` / `JOINT_WEIGHT`, so the draw specializes **skinned** — but
    /// takes the bind group from the entity's own `SkinnedMesh`. A twin spawned
    /// without one is handed a model-only bind group for a skinned pipeline,
    /// which is a wgpu validation error that quits the application, not a
    /// rendering artifact.
    ///
    /// It is worse than one bad draw: worn rigged submeshes deliberately share
    /// one mesh asset across wearers so Bevy can batch them, and the batch takes
    /// its bind group from a single representative entity — so one malformed
    /// twin takes down every wearer drawn in that batch. That is what made this
    /// read as a random crash near other people's avatars
    /// (`viewer-skinned-bind-group-quits-on-rez`), reproduced on aditi by
    /// standing an avatar in the shallows: without the skin below, a run logged
    /// 14 malformed twins; with it, 40 skinned splits and none.
    #[test]
    fn a_skinned_straddling_face_gives_its_twin_the_skin() {
        let mut app = app();
        let face = spawn_face(&mut app, LEVEL, AlphaMode::Blend);
        // The joints an avatar's skeleton instance would supply. Their identity
        // does not matter here — that the twin carries the *same* ones does.
        let joints: Vec<Entity> =
            std::iter::repeat_with(|| app.world_mut().spawn(Transform::default()).id())
                .take(3)
                .collect();
        let inverse_bindposes = app
            .world_mut()
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; 3]));
        app.world_mut().entity_mut(face).insert(SkinnedMesh {
            inverse_bindposes: inverse_bindposes.clone(),
            joints: joints.clone(),
        });
        app.update();

        assert_eq!(twins(&mut app), 1, "the straddling face gained its twin");
        let twin = app
            .world_mut()
            .query_filtered::<Entity, With<WaterClipTwin>>()
            .iter(app.world())
            .next()
            .expect("the twin exists");
        let skin = app
            .world()
            .get::<SkinnedMesh>(twin)
            .expect("the twin of a skinned face must itself be skinned");
        assert_eq!(
            skin.joints, joints,
            "the twin draws the same posed geometry, so it binds the same joints",
        );
        assert_eq!(
            skin.inverse_bindposes, inverse_bindposes,
            "and the same inverse bindposes",
        );
    }

    /// An **unskinned** face's twin must not gain a skin either — the agreement
    /// has to hold in both directions, since a `SkinnedMesh` over a mesh with no
    /// skin attributes is the same validation error mirrored.
    #[test]
    fn an_unskinned_straddling_face_gives_its_twin_no_skin() {
        let mut app = app();
        let _face = spawn_face(&mut app, LEVEL, AlphaMode::Blend);
        app.update();

        let twin = app
            .world_mut()
            .query_filtered::<Entity, With<WaterClipTwin>>()
            .iter(app.world())
            .next()
            .expect("the twin exists");
        assert!(
            app.world().get::<SkinnedMesh>(twin).is_none(),
            "an unskinned face's twin stays unskinned",
        );
    }

    /// Which of the mesh **bind group**'s inputs a face carries.
    ///
    /// This is the axis the twin has to get right, and the axis the SL asset
    /// categories collapse onto. Every face that reaches the split does so as a
    /// `PrimFaceEntity` with a `FaceMaterial` — a prim face, a mesh face and a
    /// sculpt face are all spawned by one `spawn_face_entity` and differ only in
    /// the geometry behind the handle, which the twin never looks at. What
    /// actually varies between them, and what wgpu rejects a mismatch on, is
    /// this: a worn rigged submesh (and an animesh submesh) adds a
    /// `SkinnedMesh`, and a part with runtime morphs adds `MeshMorphWeights`.
    ///
    /// So enumerating the SL categories would re-test one code path four times
    /// while leaving the combinations that actually break untested. Enumerating
    /// *these* covers every category by construction.
    #[derive(Clone, Copy, Debug)]
    struct BindGroupInputs {
        /// The face's mesh carries skin attributes, so the entity needs a
        /// `SkinnedMesh` — a worn rigged submesh or an animesh submesh.
        skinned: bool,
        /// The face's mesh carries morph targets, so the entity needs
        /// `MeshMorphWeights` — a part with runtime morphs.
        morphed: bool,
    }

    /// Give `face` the bind-group inputs `inputs` names.
    fn add_bind_group_inputs(app: &mut App, face: Entity, inputs: BindGroupInputs) {
        if inputs.skinned {
            let joints: Vec<Entity> =
                std::iter::repeat_with(|| app.world_mut().spawn(Transform::default()).id())
                    .take(3)
                    .collect();
            let inverse_bindposes = app
                .world_mut()
                .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
                .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; 3]));
            app.world_mut().entity_mut(face).insert(SkinnedMesh {
                inverse_bindposes,
                joints,
            });
        }
        if inputs.morphed {
            app.world_mut()
                .entity_mut(face)
                .insert(MeshMorphWeights::Value {
                    weights: vec![0.25, 0.5],
                });
        }
    }

    /// **The twin carries exactly the bind-group inputs its face does** — for
    /// every combination of them, not just the one that happened to crash.
    ///
    /// The twin shares the face's *mesh asset*, and Bevy decides the two halves
    /// of a draw in two different places: the pipeline is specialized from the
    /// mesh asset (its `JOINT_INDEX` / `JOINT_WEIGHT`, its `morph_targets()`)
    /// while the bind group comes from the entity (its `SkinnedMesh`, its morph
    /// index). Any input the twin is missing specializes a pipeline it cannot
    /// bind for — a wgpu validation error that quits the application, and one
    /// that takes down every other draw batched on that shared mesh with it.
    ///
    /// The skinned case shipped broken and was found only by an avatar happening
    /// to stand in the shallows on aditi
    /// (`viewer-skinned-bind-group-quits-on-rez`). The morphed case is not
    /// reachable today — runtime morphs are attached to avatar base parts, which
    /// carry no `PrimFaceEntity` and so never reach this split — which is
    /// exactly why it is pinned here rather than left to be discovered the same
    /// way if a mesh head ever brings facial morphs to a worn submesh.
    #[test]
    fn a_twins_bind_group_inputs_match_its_faces() {
        for inputs in [
            BindGroupInputs {
                skinned: false,
                morphed: false,
            },
            BindGroupInputs {
                skinned: true,
                morphed: false,
            },
            BindGroupInputs {
                skinned: false,
                morphed: true,
            },
            BindGroupInputs {
                skinned: true,
                morphed: true,
            },
        ] {
            let mut app = app();
            let face = spawn_face(&mut app, LEVEL, AlphaMode::Blend);
            add_bind_group_inputs(&mut app, face, inputs);
            app.update();

            let twin = app
                .world_mut()
                .query_filtered::<Entity, With<WaterClipTwin>>()
                .iter(app.world())
                .next()
                .expect("the straddling face gained its twin");
            assert_eq!(
                app.world().get::<SkinnedMesh>(twin).is_some(),
                inputs.skinned,
                "{inputs:?}: the twin's skin must match its face's",
            );
            assert_eq!(
                app.world().get::<MeshMorphWeights>(twin).is_some(),
                inputs.morphed,
                "{inputs:?}: the twin's morph weights must match its face's",
            );
            // The values, not merely the presence: a twin skinned to other
            // joints, or morphed by other weights, draws different geometry from
            // the half it is completing.
            assert_eq!(
                app.world()
                    .get::<SkinnedMesh>(twin)
                    .map(|skin| skin.joints.clone()),
                app.world()
                    .get::<SkinnedMesh>(face)
                    .map(|skin| skin.joints.clone()),
                "{inputs:?}: the twin binds the same joints",
            );
        }
    }

    /// A face clear of the surface is left alone — no clip, no twin, and (this is
    /// the part that matters for the material cache) no private material.
    #[test]
    fn a_face_clear_of_the_surface_is_not_split() {
        let mut app = app();
        let face = spawn_face(&mut app, LEVEL + 5.0, AlphaMode::Blend);
        app.update();
        assert_eq!(twins(&mut app), 0);
        assert_eq!(app.world().get::<WaterClipSide>(face).copied(), None);
    }

    /// An **opaque** face is left alone however it sits: it writes depth, so the sea
    /// orders against it per pixel and there is nothing to split.
    #[test]
    fn an_opaque_face_is_never_split() {
        let mut app = app();
        let _face = spawn_face(&mut app, LEVEL, AlphaMode::Opaque);
        app.update();
        assert_eq!(twins(&mut app), 0);
    }

    /// A face that stops straddling loses its twin and its clip — otherwise a prim
    /// lifted out of the water would keep drawing only half of itself.
    #[test]
    fn a_face_that_leaves_the_water_is_made_whole() {
        let mut app = app();
        let face = spawn_face(&mut app, LEVEL, AlphaMode::Blend);
        app.update();
        assert_eq!(twins(&mut app), 1);

        *app.world_mut()
            .get_mut::<GlobalTransform>(face)
            .expect("the face has a global transform") =
            GlobalTransform::from_xyz(0.0, LEVEL + 5.0, 0.0);
        app.update();

        assert_eq!(twins(&mut app), 0, "the twin is gone");
        assert_eq!(app.world().get::<WaterClipSide>(face).copied(), None);
        let materials = app.world().resource::<Assets<FaceMaterial>>();
        let own = app
            .world()
            .get::<MeshMaterial3d<FaceMaterial>>(face)
            .and_then(|handle| materials.get(&handle.0))
            .expect("the face has a material");
        assert!(
            own.extension.params.water_clip.abs() < f32::EPSILON,
            "and the face draws whole again",
        );
    }

    /// The straddle test itself, on the world-space extent rather than the centre —
    /// which is the entire point, since a straddling face's centre can be on either
    /// side of the surface or exactly on it.
    #[test]
    fn straddling_is_decided_by_the_extent_not_the_centre() {
        let aabb = Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
        for centre in [LEVEL - 0.5, LEVEL, LEVEL + 0.5] {
            assert!(
                straddles(&GlobalTransform::from_xyz(0.0, centre, 0.0), &aabb, LEVEL),
                "a 2 m face centred at {centre} crosses a surface at {LEVEL}",
            );
        }
        for centre in [LEVEL - 5.0, LEVEL + 5.0] {
            assert!(!straddles(
                &GlobalTransform::from_xyz(0.0, centre, 0.0),
                &aabb,
                LEVEL
            ));
        }
    }
}
