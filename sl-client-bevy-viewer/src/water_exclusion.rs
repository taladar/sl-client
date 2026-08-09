//! Water-exclusion surfaces (`viewer-water-exclusion`): the modern successor to
//! the legacy "invisiprim". A prim face textured with one of the alpha-gradient
//! sentinels ([`sl_client_bevy::TextureFace::is_water_exclusion`]) punches a hole
//! in the water plane where it is, so boat / dock content keeps its hull interior
//! dry. This ports the reference viewer's `LLDrawPoolWaterExclusion` /
//! `doWaterExclusionMask` / `exclusionTex` mechanism.
//!
//! **The reference.** The reference repurposed the old invisiprim draw pool
//! (`PASS_INVISIBLE`) into a dedicated water-exclusion pass. Just before the water
//! is drawn it renders the exclusion faces into a screen-space `R8`
//! `mWaterExclusionMask` (cleared white = "water here", faces drawn black), then
//! the water fragment shader samples that mask and `discard`s the sea where it
//! reads black (`class3/environment/waterF.glsl`, `if (water_mask < 1) discard`).
//! Crucially the modern pass only excludes **water** — unlike the legacy
//! invisiprim it no longer occludes avatars, objects, or the sky. This viewer
//! follows the modern reference exactly (water exclusion only), per the
//! support-legacy-content policy of matching today's reference: the legacy
//! avatar/sky occlusion is deliberately **not** reproduced.
//!
//! **This port.** Bevy 0.19's renderer has no render-graph, and the water surface
//! is drawn in the main pass's transparent phase, so a `PostProcess`-style system
//! would run too late to feed it. Instead a dedicated **mask camera** — slaved to
//! the main [`ViewerCamera`]'s pose and projection and rendered first
//! (`order = -1`) into an `R8` [`Image`] target — renders only the exclusion faces
//! (routed onto [`WATER_EXCLUSION_LAYER`], invisible to every ordinary view) as
//! flat black on a white clear. That image is bound into the shared
//! [`WaterMaterial`]'s `exclusion_mask` slot, and `water.wgsl` samples it by the
//! fragment's screen position and discards the sea where it reads black. Because
//! the mask is a 2-D silhouette (rendered double-sided), it excludes the sea from
//! every viewing angle, including looking down into an open hull.
//!
//! **Simplification vs. the reference.** The reference depth-tests the exclusion
//! faces against the scene depth so a hull hidden behind opaque geometry does not
//! mark the mask. The mask camera here has its own (exclusion-only) depth buffer
//! and cannot read the main scene depth before the main pass runs, so an exclusion
//! surface occluded by nearer opaque geometry still marks the mask — wrongly
//! excluding water beyond it. That needs an exclusion surface to sit behind opaque
//! geometry *and* have visible water past it, a rare combination; documented and
//! left for a later pass.

use bevy::app::Propagate;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::window::PrimaryWindow;
use sl_client_bevy::WaterMaterial;

use crate::camera::ViewerCamera;
use crate::face_material::FaceMaterial;
use crate::material_cache::SharedFaceMaterial;
use crate::objects::FaceTextureDebug;
use crate::probe_layers::WATER_EXCLUSION_LAYER;
use crate::water::WaterState;

/// The mask camera's render order: below the main camera's default `0` so the mask
/// is finished before the main pass's water fragments sample it (the same slot the
/// reflection-probe capture cameras use — they target their own images, so there
/// is no same-target ordering ambiguity).
const MASK_CAMERA_ORDER: isize = -1;

/// The mask render-target size used before the first window is seen (a harmless
/// fallback; [`sync_water_exclusion_camera`] resizes it to the window each frame).
const FALLBACK_MASK_SIZE: (u32, u32) = (1280, 720);

/// Marks a face entity that has been converted into a water-exclusion surface, so
/// [`convert_water_exclusion_faces`] processes each face only once.
#[derive(Component)]
pub(crate) struct WaterExclusionFace;

/// Marks the mask camera, so [`sync_water_exclusion_camera`] can slave it to the
/// main [`ViewerCamera`].
#[derive(Component)]
pub(crate) struct WaterExclusionCamera;

/// The water-exclusion render assets: the screen-space mask [`Image`] the mask
/// camera draws into and the water shader samples, and the flat-black material
/// every exclusion face wears in that mask pass.
#[derive(Resource)]
pub(crate) struct WaterExclusionMask {
    /// The `R8` mask render target (white = water, black = exclusion), sized to the
    /// window by [`sync_water_exclusion_camera`].
    image: Handle<Image>,
    /// The flat-black unlit material an exclusion face renders with on the mask
    /// camera (shared by every exclusion face).
    material: Handle<StandardMaterial>,
}

/// Startup: create the mask render target and the flat-black exclusion material,
/// and spawn the mask camera (slaved to the main view by
/// [`sync_water_exclusion_camera`]).
pub(crate) fn setup_water_exclusion(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let (width, height) = windows.single().map_or(FALLBACK_MASK_SIZE, |window| {
        (
            window.physical_width().max(1),
            window.physical_height().max(1),
        )
    });
    // A single-channel mask: 1 (white clear) = water present, 0 (black faces) =
    // exclusion. `new_target_texture` sets the render-attachment + sampled usages.
    let image = images.add(Image::new_target_texture(
        width,
        height,
        TextureFormat::R8Unorm,
        None,
    ));
    // Flat black, unlit, double-sided: an exclusion face fills its whole silhouette
    // in the mask (both hull sides) regardless of the (unlit) lighting, matching the
    // reference's double-sided invisiprim batches (`LLGLDisable cullface`).
    let material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });

    commands.spawn((
        WaterExclusionCamera,
        Camera3d::default(),
        Camera {
            // White clear = "water everywhere"; the exclusion faces paint the holes.
            clear_color: ClearColorConfig::Custom(Color::WHITE),
            order: MASK_CAMERA_ORDER,
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        // Matches the main camera's near/far; the fov and aspect are slaved each
        // frame by `sync_water_exclusion_camera` (and Bevy derives the aspect from
        // the window-sized target).
        Projection::Perspective(PerspectiveProjection {
            near: 0.02,
            far: 4096.0,
            ..default()
        }),
        Transform::default(),
        // Only the exclusion faces live on this layer, so this camera renders
        // nothing else, and no ordinary view renders the exclusion faces.
        RenderLayers::layer(WATER_EXCLUSION_LAYER),
        // The target is single-sampled.
        Msaa::Off,
        // No tone mapping: the mask is a raw coverage signal, not a colour image.
        Tonemapping::None,
        Name::new("water-exclusion-mask-camera"),
    ));

    commands.insert_resource(WaterExclusionMask { image, material });
}

/// Convert a newly-spawned or re-textured face into a water-exclusion surface: drop
/// its visible material and route it onto [`WATER_EXCLUSION_LAYER`] wearing the
/// flat-black mask material, so it is invisible in every ordinary view and shows up
/// only in the mask.
///
/// Runs on `Changed<FaceTextureDebug>` (which includes the initial spawn), so it
/// catches a face that ships as an exclusion surface as well as one scripted into
/// the invisiprim texture. Reverting an exclusion face *back* to an ordinary
/// texture in place is not handled (it would need the full material rebuild path) —
/// a rare case, documented; a re-rez rebuilds the face and picks up the change.
///
/// The conversion is queued as a world command that re-checks the entity exists at
/// apply time: an object rebuild despawns and respawns its faces on an update, so a
/// face this query matched can be despawned by `update_objects` before the command
/// buffer flushes (both run in `Update`); a plain `commands.entity(..)` would then
/// panic on the dead entity. The respawned face is caught again next frame.
#[expect(
    clippy::type_complexity,
    reason = "a Bevy query of newly-textured faces not yet converted to exclusion surfaces"
)]
pub(crate) fn convert_water_exclusion_faces(
    mut commands: Commands,
    faces: Query<
        (Entity, &FaceTextureDebug),
        (Changed<FaceTextureDebug>, Without<WaterExclusionFace>),
    >,
    mask: Option<Res<WaterExclusionMask>>,
) {
    let Some(mask) = mask else {
        return;
    };
    for (entity, debug) in &faces {
        if !debug.0.is_water_exclusion() {
            continue;
        }
        let material = mask.material.clone();
        commands.queue(move |world: &mut World| {
            let Ok(mut face) = world.get_entity_mut(entity) else {
                // Despawned between the query and now (object rebuilt this frame);
                // the respawned face is converted next frame.
                return;
            };
            face.remove::<MeshMaterial3d<FaceMaterial>>();
            face.remove::<SharedFaceMaterial>();
            face.insert((
                MeshMaterial3d(material),
                // A face's own `Propagate` overrides the layer its object root
                // propagates down (the HUD-attachment precedent), so this face
                // leaves the main / probe layers for the exclusion layer alone.
                Propagate(RenderLayers::layer(WATER_EXCLUSION_LAYER)),
                WaterExclusionFace,
            ));
        });
    }
}

/// Slave the mask camera to the main [`ViewerCamera`] (pose + projection) and keep
/// the mask target sized to the window, so the mask lines up pixel-for-pixel with
/// the main view the water samples it against.
#[expect(
    clippy::type_complexity,
    reason = "a Bevy query pairing the main camera's pose and projection, kept disjoint \
              from the mask camera it is copied onto"
)]
pub(crate) fn sync_water_exclusion_camera(
    main: Query<
        (&GlobalTransform, &Projection),
        (With<ViewerCamera>, Without<WaterExclusionCamera>),
    >,
    mut mask_camera: Query<(&mut Transform, &mut Projection), With<WaterExclusionCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mask: Option<Res<WaterExclusionMask>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Ok((main_global, main_projection)), Ok((mut mask_transform, mut mask_projection))) =
        (main.single(), mask_camera.single_mut())
    else {
        return;
    };
    // Guard both copies so a parked camera stops dirtying the mask camera every
    // frame (an unconditional write re-extracts its view each frame).
    // `Projection` has no `PartialEq` (its `Custom` variant is a boxed trait
    // object), so compare the clip matrices — they capture everything the mask
    // render sees of the projection.
    mask_transform.set_if_neq(Transform::from_matrix(main_global.to_matrix()));
    if mask_projection.get_clip_from_view() != main_projection.get_clip_from_view() {
        *mask_projection = main_projection.clone();
    }

    let (Some(mask), Ok(window)) = (mask, windows.single()) else {
        return;
    };
    let (width, height) = (
        window.physical_width().max(1),
        window.physical_height().max(1),
    );
    if let Some(mut image) = images.get_mut(&mask.image)
        && (image.width() != width || image.height() != height)
    {
        image.resize(Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        });
    }
}

/// Bind the real mask [`Image`] into the shared [`WaterMaterial`] once both the
/// water material and the mask exist, replacing the white "water everywhere"
/// placeholder [`setup_water`](crate::water::setup_water) seeded it with. A one-
/// shot: the mask target is stable, so once bound there is nothing to update.
pub(crate) fn bind_water_exclusion_mask(
    water: Option<Res<WaterState>>,
    mask: Option<Res<WaterExclusionMask>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut bound: Local<bool>,
) {
    if *bound {
        return;
    }
    let (Some(water), Some(mask)) = (water, mask) else {
        return;
    };
    if let Some(mut material) = materials.get_mut(water.material()) {
        material.exclusion_mask = mask.image.clone();
        *bound = true;
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::Propagate;
    use bevy::camera::visibility::RenderLayers;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{TextureKey, Uuid};
    use sl_proto::{IMG_ALPHA_GRAD, TextureFace};

    use super::{WaterExclusionFace, WaterExclusionMask, convert_water_exclusion_faces};
    use crate::face_material::FaceMaterial;
    use crate::material_cache::SharedFaceMaterial;
    use crate::objects::FaceTextureDebug;
    use crate::probe_layers::WATER_EXCLUSION_LAYER;

    /// A face carrying the invisiprim-successor sentinel is converted into a
    /// water-exclusion surface: it loses its visible material, gains the flat-black
    /// mask material, is routed onto [`WATER_EXCLUSION_LAYER`] via its own
    /// `Propagate`, and is marked so it is only processed once. A plain face is left
    /// untouched.
    #[test]
    fn converts_only_sentinel_faces() {
        let mut app = App::new();
        app.insert_resource(WaterExclusionMask {
            image: Handle::default(),
            material: Handle::default(),
        });
        app.add_systems(Update, convert_water_exclusion_faces);

        // An exclusion face (ships with the invisiprim-successor sentinel) and a
        // plain textured face, both spawned as ordinary faces first.
        let exclusion = app
            .world_mut()
            .spawn((
                FaceTextureDebug(TextureFace::new(TextureKey::from(IMG_ALPHA_GRAD))),
                MeshMaterial3d(Handle::<FaceMaterial>::default()),
                SharedFaceMaterial,
            ))
            .id();
        let plain = app
            .world_mut()
            .spawn((
                FaceTextureDebug(TextureFace::new(TextureKey::from(Uuid::from_u128(0xabcd)))),
                MeshMaterial3d(Handle::<FaceMaterial>::default()),
            ))
            .id();

        app.update();

        // The exclusion face is diverted: no visible material, the mask material and
        // the exclusion layer, and the once-only marker.
        let exclusion_ref = app.world().entity(exclusion);
        assert!(exclusion_ref.contains::<WaterExclusionFace>());
        assert!(exclusion_ref.contains::<MeshMaterial3d<StandardMaterial>>());
        assert!(!exclusion_ref.contains::<MeshMaterial3d<FaceMaterial>>());
        assert!(!exclusion_ref.contains::<SharedFaceMaterial>());
        assert_eq!(
            exclusion_ref
                .get::<Propagate<RenderLayers>>()
                .map(|propagate| propagate.0.clone()),
            Some(RenderLayers::layer(WATER_EXCLUSION_LAYER)),
        );

        // The plain face is untouched.
        let plain_ref = app.world().entity(plain);
        assert!(!plain_ref.contains::<WaterExclusionFace>());
        assert!(plain_ref.contains::<MeshMaterial3d<FaceMaterial>>());
    }
}
