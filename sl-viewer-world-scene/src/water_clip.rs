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
/// crosses the surface), the clip it already carries, and whether its material is
/// one the [`MaterialCache`](sl_viewer_world_objects::material_cache) shares.
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
    for (face, mesh, material, transform, aabb, side, shared) in &faces {
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
            "split face {face}: straddles the surface at {level} m, drawing it twice",
        );
        commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(twin),
            Transform::IDENTITY,
            WaterClipTwin,
            WaterClipSide::Below,
            ChildOf(face),
        ));
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
